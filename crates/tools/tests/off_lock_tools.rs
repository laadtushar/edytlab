//! The list of tools that run without the store's lock held (#421).
//!
//! `Tool::runs_off_the_lock` defaults to `false`: a new tool, and every tool
//! an MCP server adds, runs under the store, engine and clipboard locks as
//! every tool did before. A tool that says `true` is run on a staged store
//! handle by `Shared::dispatch`, and run again on a newer head if the user
//! edited while it ran. That is only correct for a tool that cannot tell
//! the difference, so the list is pinned here by name, and each tool on it
//! is checked for the things a staged run cannot do. Widening the list
//! shows up as a diff of this file, with a reason, instead of arriving as a
//! one-line override in the middle of a tool.

use std::path::PathBuf;

use serde_json::Value;
use tools::schema::{anthropic_tool, object_schema};
use tools::{Tool, ToolContext, ToolDispatcher, ToolResult};

/// Every tool that runs off the lock, alphabetical.
///
/// Deliberately **not** on this list:
///
/// * `copy_region` and `paste_region` use the clipboard, which an off-lock
///   run does not have.
/// * `render_preview`, `audition_effect` and `render_final` stage their
///   output in the preview cache under `.{node}.partial`, a fixed name
///   shared with the `render_preview` command, which only the engine lock
///   serialises today. Follow-up: unique staging names, then opt in.
/// * `batch_apply` and `apply_recipe` build nested dispatchers and report
///   `progress`; they would need a commit per step.
/// * `compact_session`, `storage_report`, `fork_node`, `revert_to`,
///   `name_node` and `apply_diff` read or rewrite the history itself.
/// * `transcribe`, `separate_stems` and `split_by_speaker` need models
///   that are not shipped.
/// * `select_region` reports `progress`.
/// * Metadata tools (`set_track_gain`, `mute_track`, ...), `generate_*`,
///   `export_*`, `import_labels`, `label`, `cut_range`, `trim`, `gain` and
///   MCP tools are fast, or have not been reviewed for a re-run on a newer
///   head. Possible follow-ups, one reviewed decision each.
const OFF_LOCK: [&str; 40] = [
    "align_to_beat",
    "analyze_track",
    "change_speed",
    "click_removal",
    "compressor",
    "de_esser",
    "distortion",
    "echo",
    "eq",
    "fade",
    "high_pass_filter",
    "insert_silence",
    "invert",
    "leveler",
    "limiter",
    "load",
    "low_pass_filter",
    "mix_to_new_track",
    "mono_to_stereo",
    "noise_gate",
    "noise_reduction",
    "normalize",
    "normalize_loudness",
    "notch_filter",
    "phaser",
    "pitch_shift",
    "plot_spectrum",
    "punch_in",
    "repeat_selection",
    "resample_track",
    "reverb",
    "reverse",
    "silence_finder",
    "silence_region",
    "stereo_to_mono",
    "stereo_widener",
    "time_stretch",
    "tremolo",
    "truncate_silence",
    "vocal_reduction",
];

#[test]
fn the_off_lock_list_is_pinned() {
    let dispatcher = ToolDispatcher::default_dispatcher();
    let mut actual: Vec<String> = dispatcher
        .tool_names()
        .into_iter()
        .filter(|name| {
            dispatcher
                .get(name)
                .expect("registered")
                .runs_off_the_lock()
        })
        .collect();
    actual.sort();

    assert_eq!(
        actual,
        OFF_LOCK.to_vec(),
        "the set of tools that run without the store's lock changed.\n\
         A tool on this list is run on a staged store handle and run again on \
         a newer head if the user edits while it runs. That must be a reviewed \
         decision: if it is intended, update OFF_LOCK in this file, with the \
         reason, in the same change, and make sure \
         `every_off_lock_tool_avoids_what_a_staged_run_cannot_do` still holds."
    );
}

/// A tool that says nothing about it runs under the lock. This is the
/// property the whole design rests on, so it is checked against a bare
/// implementation rather than against a tool that happens not to
/// override it.
#[test]
fn a_tool_runs_under_the_lock_unless_it_says_otherwise() {
    struct Bare;
    impl Tool for Bare {
        fn name(&self) -> &'static str {
            "bare"
        }
        fn schema(&self) -> Value {
            anthropic_tool("bare", "fixture", object_schema(&[]))
        }
        fn invoke(&self, _args: Value, _ctx: &mut ToolContext) -> tools::Result<ToolResult> {
            Ok(ToolResult::Ok(Value::Null))
        }
    }
    assert!(!Bare.runs_off_the_lock());
}

/// What a staged run cannot do, as the text it would be written with.
/// Method calls are matched with their dot and parenthesis, so a variable
/// called `preset_label` is not taken for `set_label`.
///
/// Only code is matched: comments and the inside of string and character
/// literals are blanked first (see [`code_of`]), so a tool's description
/// may talk about the clipboard.
const FORBIDDEN: &[(&str, &str)] = &[
    (
        "clipboard",
        "the clipboard is a local empty one in an off-lock run",
    ),
    (
        "Clipboard",
        "the clipboard is a local empty one in an off-lock run",
    ),
    (
        "ToolDispatcher",
        "a nested dispatcher runs its tools under this one's context",
    ),
    (
        "default_dispatcher",
        "a nested dispatcher runs its tools under this one's context",
    ),
    (
        ".engine",
        "the engine is a fresh one in an off-lock run, however the field is reached",
    ),
    (
        "ToolContext {",
        "taking the context apart, or building one, reaches the engine and clipboard without naming them",
    ),
    (
        "ToolContext{",
        "taking the context apart, or building one, reaches the engine and clipboard without naming them",
    ),
    (
        "Store::open",
        "a second handle on the store bypasses staging",
    ),
    (
        "Store as",
        "an alias for the store hides `Store::open` from this scan",
    ),
    (
        "rederive",
        "re-deriving opens a second store and builds a nested dispatcher",
    ),
    (
        "materialize",
        "materializing opens a second store and builds a nested dispatcher",
    ),
    ("recipe::", "a recipe runs a nested dispatcher"),
    (
        "reclaim::",
        "a sweep deletes files a staged run depends on",
    ),
    (".set_head(", "a staged store refuses to move the head"),
    (".set_label(", "a staged store refuses to rewrite a node"),
    (".remove_node(", "a staged store refuses to delete history"),
    (
        ".detach_parent(",
        "a staged store refuses to rewrite history",
    ),
    (
        ".append_branches(",
        "a staged store refuses to write branches",
    ),
    (".fork(", "forking moves the head"),
    (".revert_to(", "reverting is a history operation"),
    (".list_nodes(", "scanning every node is a history operation"),
    (
        "progress::",
        "progress is reported against the caller's context",
    ),
    (
        "std::process",
        "a tool that spawns work outside the call cannot be re-run safely",
    ),
];

/// The parts of the crate a tool that runs off the lock may reach with a
/// `crate::` path. Anything else is a module nobody has read for what a
/// staged run cannot do, which is how a helper in another file (the
/// `rederive` module opens a second `Store` and builds a dispatcher)
/// would get past a scan of the tool's own source. A path may start with
/// any entry; one that starts with none is reported. Widening this list is
/// a reviewed decision like widening `OFF_LOCK`: read the module first.
const REACHABLE: &[&str] = &[
    "tool::util",
    "util::range_resolver",
    "schema",
    "provenance",
    "dispatcher",
];

/// What may be named at the crate root: the tool traits and results, and
/// `Range`, which `util::range_resolver` (above) re-exports.
const ROOT_ITEMS: &[&str] = &["Tool", "ToolContext", "ToolResult", "Result", "Range"];

fn source_of(file: &str) -> String {
    let path: PathBuf = [env!("CARGO_MANIFEST_DIR"), "src", "tool", file]
        .iter()
        .collect();
    assert!(
        path.is_file(),
        "{file} is not under src/tool/, so it cannot be scanned. \
         A tool that lives elsewhere needs this test taught where."
    );
    std::fs::read_to_string(&path).expect("readable source")
}

/// `source` with comments and the inside of string and character literals
/// replaced by spaces, newlines kept: what is left is the code, and braces
/// that are code. Handles line and nested block comments, strings with
/// escapes, raw strings with any number of hashes, character literals, and
/// lifetimes (which look like the start of one).
fn blank_literals(source: &str) -> String {
    let chars: Vec<char> = source.chars().collect();
    let mut out = String::with_capacity(source.len());
    let blank = |c: char| if c == '\n' { '\n' } else { ' ' };
    let is_ident = |c: char| c.is_alphanumeric() || c == '_';
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        // Line comment.
        if c == '/' && next == Some('/') {
            while i < chars.len() && chars[i] != '\n' {
                out.push(' ');
                i += 1;
            }
            continue;
        }
        // Block comment, which nests.
        if c == '/' && next == Some('*') {
            let mut depth = 0;
            while i < chars.len() {
                if chars[i] == '/' && chars.get(i + 1) == Some(&'*') {
                    depth += 1;
                    out.push_str("  ");
                    i += 2;
                } else if chars[i] == '*' && chars.get(i + 1) == Some(&'/') {
                    depth -= 1;
                    out.push_str("  ");
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    out.push(blank(chars[i]));
                    i += 1;
                }
            }
            continue;
        }
        // Raw string: r"..", r#".."#, br"..", with the closing quote
        // followed by as many hashes as opened it.
        let prev_is_ident = i > 0 && is_ident(chars[i - 1]);
        if !prev_is_ident && (c == 'r' || (c == 'b' && next == Some('r'))) {
            let mut j = i + usize::from(c == 'b') + 1;
            let mut hashes = 0;
            while chars.get(j) == Some(&'#') {
                hashes += 1;
                j += 1;
            }
            if chars.get(j) == Some(&'"') {
                // Keep the opening as it is, blank the rest through the close.
                out.extend(chars[i..=j].iter().map(|&ch| blank(ch)));
                i = j + 1;
                while i < chars.len() {
                    let closes =
                        chars[i] == '"' && (1..=hashes).all(|k| chars.get(i + k) == Some(&'#'));
                    if closes {
                        out.push_str(&" ".repeat(hashes + 1));
                        i += hashes + 1;
                        break;
                    }
                    out.push(blank(chars[i]));
                    i += 1;
                }
                continue;
            }
        }
        // String.
        if c == '"' {
            out.push(' ');
            i += 1;
            while i < chars.len() && chars[i] != '"' {
                if chars[i] == '\\' && i + 1 < chars.len() {
                    out.push(' ');
                    i += 1;
                }
                out.push(blank(chars[i]));
                i += 1;
            }
            out.push(' ');
            i += 1;
            continue;
        }
        // Character literal, or a lifetime: `'x'` and `'\n'` are the
        // first; `'a` with no closing quote after one character is the
        // second and is left alone.
        if c == '\'' {
            let literal_end = match (next, chars.get(i + 2)) {
                // After the backslash and the character it escapes, up to
                // the closing quote: `'\n'`, `'\''`, `'\\'`, `'\u{41}'`.
                (Some('\\'), _) => (i + 3..chars.len()).find(|&k| chars[k] == '\''),
                (Some(_), Some('\'')) => Some(i + 2),
                _ => None,
            };
            if let Some(end) = literal_end {
                out.extend(chars[i..=end].iter().map(|&ch| blank(ch)));
                i = end + 1;
                continue;
            }
        }
        out.push(c);
        i += 1;
    }
    out
}

/// Whether a line opens an item that exists only in tests.
fn is_cfg_test(line: &str) -> bool {
    let line = line.trim_start();
    line.starts_with("#[cfg(test)]")
        || line.starts_with("#[cfg(all(test")
        || line.starts_with("#[cfg(any(test")
}

/// The lines of a source file that are code a tool runs: with their
/// numbers, comments and the inside of literals blanked, and every item
/// marked `#[cfg(test)]` left out — that item only: a module of tests, a
/// test-only `use` or function, up to where it ends, and then the scan goes
/// on. (A scan that stopped at the first `#[cfg(test)]` stopped looking at
/// anything after it.)
fn code_of(source: &str) -> Vec<(usize, String)> {
    let blanked = blank_literals(source);
    let mut out = Vec::new();
    // Inside a test-only item: how deep in braces, brackets and
    // parentheses, and whether its body has opened.
    let mut skipping: Option<(i32, i32, bool)> = None;
    for (i, blanked_line) in blanked.lines().enumerate() {
        if skipping.is_none() && is_cfg_test(blanked_line) {
            skipping = Some((0, 0, false));
        }
        if let Some((braces, nest, opened)) = skipping.as_mut() {
            for c in blanked_line.chars() {
                match c {
                    '{' => {
                        *braces += 1;
                        *opened = true;
                    }
                    '}' => *braces -= 1,
                    '(' | '[' => *nest += 1,
                    ')' | ']' => *nest -= 1,
                    // `#[cfg(test)] use x;` ends at its semicolon, if it
                    // is not one inside a type like `[u8; 4]`.
                    ';' if !*opened && *nest == 0 => *opened = true,
                    _ => {}
                }
            }
            if *opened && *braces <= 0 {
                skipping = None;
            }
            continue;
        }
        out.push((i + 1, blanked_line.to_string()));
    }
    out
}

/// Every `crate::` path in `code`, as its text after `crate::` up to the
/// end of the path: `tool::util::flatten`, or for a group, `{Tool, ToolResult}`.
fn crate_paths(code: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = code;
    while let Some(at) = rest.find("crate::") {
        let after = &rest[at + "crate::".len()..];
        let end = if after.starts_with('{') {
            after.find('}').map_or(after.len(), |e| e + 1)
        } else {
            after
                .find(|c: char| !(c.is_alphanumeric() || c == '_' || c == ':'))
                .unwrap_or(after.len())
        };
        out.push(after[..end].to_string());
        rest = &after[end..];
    }
    out
}

/// Whether a `crate::` path stays inside [`REACHABLE`] and [`ROOT_ITEMS`].
fn reachable(path: &str) -> bool {
    if let Some(group) = path.strip_prefix('{') {
        return group
            .trim_end_matches('}')
            .split(',')
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .all(|item| ROOT_ITEMS.contains(&item));
    }
    // An item at the crate root, with or without a path below it
    // (`ToolResult::Error`).
    let first = path.split("::").next().unwrap_or_default();
    ROOT_ITEMS.contains(&first)
        || REACHABLE
            .iter()
            .any(|ok| path == *ok || path.starts_with(&format!("{ok}::")))
}

fn offences_in(file: &str, source: &str) -> Vec<String> {
    let mut out = Vec::new();
    for (n, line) in code_of(source) {
        for &(token, why) in FORBIDDEN {
            if line.contains(token) {
                out.push(format!("{file}:{n}: `{token}` ({why}): {}", line.trim()));
            }
        }
        for path in crate_paths(&line) {
            if !reachable(&path) {
                out.push(format!(
                    "{file}:{n}: `crate::{path}` is a module this scan has not been taught \
                     to trust (REACHABLE): {}",
                    line.trim()
                ));
            }
        }
    }
    out
}

fn offences(file: &str) -> Vec<String> {
    offences_in(file, &source_of(file))
}

/// The opt-in is a claim that the tool does only what a staged handle
/// allows. This reads each opted-in tool's source for the things it
/// cannot. It is a guard against the obvious mistake — a tool that is
/// opted in and then grows a clipboard read — not a proof; the
/// behavioural tests in `off_lock_dispatch.rs` are the proof for the tools
/// they run.
#[test]
fn every_off_lock_tool_avoids_what_a_staged_run_cannot_do() {
    let mut problems = Vec::new();
    for name in OFF_LOCK {
        problems.extend(offences(&format!("{name}.rs")));
    }
    // The helpers they share: an edit to one reaches all of them.
    problems.extend(offences("util.rs"));
    assert!(
        problems.is_empty(),
        "a tool that runs off the store lock does something a staged run cannot:\n  {}\n\n\
         Either take the tool off OFF_LOCK (and its `runs_off_the_lock`), or remove \
         the call.",
        problems.join("\n  ")
    );
}

/// The scan only means something if it can fail.
#[test]
fn the_scan_finds_what_it_looks_for() {
    let found = |source: &str| -> Vec<usize> {
        offences_in("fixture.rs", source)
            .iter()
            .map(|o| {
                o.split(':')
                    .nth(1)
                    .and_then(|n| n.parse().ok())
                    .expect("a line number")
            })
            .collect()
    };

    // A call, and the same words in a comment, a doc comment, a string and
    // a raw string, which are not calls.
    let source = "fn f() {\n    // ctx.clipboard in a comment\n    /// ctx.clipboard in docs\n    \
                  let a = \"ctx.clipboard\";\n    let b = r#\"ctx.clipboard\"#;\n    \
                  ctx.store.set_head(x);\n}\n";
    assert_eq!(found(source), vec![6]);

    // A test module is left out, and so is a test-only function and a
    // test-only `use`, with braces in strings and characters not counted.
    let source = "fn f() {}\n#[cfg(test)]\nmod tests {\n    fn g() { ctx.engine; let s = \"}\"; \
                  let c = '{'; }\n    fn h() { ctx.engine; }\n}\n";
    assert!(found(source).is_empty(), "{:?}", found(source));
    let source =
        "#[cfg(test)]\nuse tools::default_dispatcher;\nfn f() { ctx.store.set_head(x); }\n";
    assert_eq!(found(source), vec![3]);
    let source =
        "#[cfg(test)]\nfn t(a: [u8; 4]) {\n    ctx.engine;\n}\nfn f() { ctx.store.set_head(x); }\n";
    assert_eq!(found(source), vec![5]);

    // What comes after a test module is still code. This is the case a scan
    // that stopped at the first `#[cfg(test)]` missed.
    let source = "fn f() {}\n#[cfg(test)]\nmod tests {\n    fn g() {}\n}\n\
                  fn later(ctx: &mut ToolContext) {\n    ctx.store.set_head(x);\n}\n\
                  #[cfg(test)]\nmod more {\n    fn h() { ctx.engine; }\n}\n";
    assert_eq!(found(source), vec![7]);

    // Ways round the plain spellings.
    assert_eq!(
        found("use session::Store as S;\nfn f() { S::open(p); }\n"),
        vec![1]
    );
    assert_eq!(
        found("fn f(ctx: &mut ToolContext) {\n    let ToolContext { engine, .. } = ctx;\n}\n"),
        vec![2],
        "destructuring the context"
    );
    assert_eq!(
        found("fn f(ctx: &mut ToolContext) {\n    ctx.engine.render();\n}\n"),
        vec![2]
    );
    assert_eq!(
        found("fn f() {\n    crate::rederive::materialize(s, id);\n}\n"),
        vec![2, 2, 2],
        "the helper module, its function, and a path nobody vetted"
    );
    assert_eq!(found("fn f() { recipe::apply(x); }\n"), vec![1]);
    assert_eq!(found("fn f() { reclaim::sweep(x); }\n"), vec![1]);

    // `crate::` paths: the vetted ones pass, anything else is reported.
    assert!(found(
        "use crate::{Tool, ToolContext, ToolResult};\nuse crate::tool::util::edit;\n\
         use crate::schema::anthropic_tool;\nuse crate::util::range_resolver::resolve;\n\
         use crate::provenance::derived_dir;\n"
    )
    .is_empty());
    assert_eq!(found("use crate::batch::run;\n"), vec![1]);
    assert_eq!(found("use crate::{Tool, Shared};\n"), vec![1]);
    assert_eq!(found("use crate::tool::batch::run;\n"), vec![1]);
}

/// The guard in `Shared::dispatch` (a re-run must mean what the first run
/// meant) finds the tracks a call names by reading its arguments:
/// `tools::shared::addressed_tracks` takes every argument with `track` in
/// its name that holds a whole number or a list of them. A tool that names
/// a track some other way — an object, a string — would be re-run on a
/// session its arguments no longer describe, with no error, so every tool
/// that runs off the lock is checked to name its tracks the way the guard
/// reads them.
#[test]
fn every_off_lock_tool_names_its_tracks_where_the_dispatch_guard_looks() {
    let dispatcher = ToolDispatcher::default_dispatcher();
    let mut problems = Vec::new();
    for name in OFF_LOCK {
        let schema = dispatcher.get(name).expect("registered").schema();
        let props = schema["input_schema"]["properties"]
            .as_object()
            .cloned()
            .unwrap_or_default();
        for (arg, spec) in props.iter().filter(|(arg, _)| arg.contains("track")) {
            let whole = |t: &Value| t["type"] == "integer";
            let ok = whole(spec) || (spec["type"] == "array" && whole(&spec["items"]));
            if !ok {
                problems.push(format!(
                    "{name}: `{arg}` is {spec}, which `addressed_tracks` does not read"
                ));
            }
        }
    }
    assert!(
        problems.is_empty(),
        "an off-lock tool names a track in a way the re-run guard cannot see:\n  {}\n\n\
         Name it with an integer (or a list of integers) in an argument whose name \
         contains `track`, or teach `tools::shared::addressed_tracks` the new shape.",
        problems.join("\n  ")
    );

    // And the tools that take a track at all do: a vacuous pass would be
    // an empty list of tools or of arguments.
    let named: Vec<&str> = OFF_LOCK
        .iter()
        .copied()
        .filter(|name| {
            dispatcher.get(name).unwrap().schema()["input_schema"]["properties"]
                .as_object()
                .is_some_and(|p| p.keys().any(|k| k.contains("track")))
        })
        .collect();
    assert!(
        named.len() >= 35 && named.contains(&"time_stretch") && named.contains(&"mix_to_new_track"),
        "{named:?}"
    );
}
