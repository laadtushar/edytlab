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
/// * `render_preview`, `audition_effect` and `render_final` are not opted
///   in. `render_preview` and `audition_effect` stage their output in the
///   preview caches under unique names (#535), so the shared
///   `.{node}.partial` name that used to be a reason is gone. The engine is
///   not the open question either: `audio_engine::Engine` holds no state, so
///   a fresh one renders the same. What is open is `rederive::materialize`,
///   which `render_preview` and `render_final` call before they render: it
///   opens a second store and writes `derived/` files for a node whose audio
///   a sweep removed, from what an off-lock run would hold as a staged
///   handle. Follow-up: one reviewed decision each.
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
        "Store::{",
        "renaming the store inside a `use` group hides `Store::open` from this scan",
    ),
    (
        "Store>::",
        "a qualified path on the store hides `Store::open` from this scan",
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
/// `crate::` path, or with the `super::` path that is the same module
/// (a tool's file is in `src/tool/`, so `super::` there is `crate::tool`
/// and `super::super::` is the crate root). Anything else is a module nobody has read for what a
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

fn is_ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// The code lines of a file as one text, so that a path or an item that
/// rustfmt wraps onto the next line is still one thing, with the line each
/// offset in it came from.
struct Code<'a> {
    lines: &'a [(usize, String)],
    text: String,
    /// The offset in `text` where each of `lines` starts.
    starts: Vec<usize>,
}

impl<'a> Code<'a> {
    fn new(lines: &'a [(usize, String)]) -> Self {
        let mut text = String::new();
        let mut starts = Vec::with_capacity(lines.len());
        for (_, line) in lines {
            starts.push(text.len());
            text.push_str(line);
            text.push('\n');
        }
        Self {
            lines,
            text,
            starts,
        }
    }

    /// The number and the text of the line `offset` is on.
    fn locate(&self, offset: usize) -> (usize, &str) {
        let at = self.starts.partition_point(|&s| s <= offset).max(1) - 1;
        let (n, line) = &self.lines[at];
        (*n, line.trim())
    }
}

/// The offset of the `}` that closes the `{` at `open` in `text`.
fn closing_brace(text: &str, open: usize) -> Option<usize> {
    let mut depth = 0;
    for (i, c) in text[open..].char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(open + i);
                }
            }
            _ => {}
        }
    }
    None
}

/// `inner` (the inside of a `{ }` group) cut at the commas that are not
/// inside a group of their own.
fn split_group(inner: &str) -> Vec<&str> {
    let (mut parts, mut depth, mut from) = (Vec::new(), 0, 0);
    for (i, c) in inner.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(&inner[from..i]);
                from = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&inner[from..]);
    parts
}

/// The paths a use-tree names, one each, groups spelled out:
/// `{Tool, tool::{util, batch}}` is `Tool`, `tool::util` and `tool::batch`.
/// A rename (`util::edit as e`) is read as what it renames.
fn expand_tree(tree: &str) -> Vec<String> {
    let tree = tree.trim();
    let Some(open) = tree.find('{') else {
        return tree
            .split_whitespace()
            .next()
            .map(|path| vec![path.to_string()])
            .unwrap_or_default();
    };
    let close = closing_brace(tree, open).unwrap_or(tree.len());
    let prefix = tree[..open].trim();
    let inner = tree.get(open + 1..close).unwrap_or_default();
    split_group(inner)
        .into_iter()
        .flat_map(expand_tree)
        .map(|path| format!("{prefix}{path}"))
        .collect()
}

/// Every `crate::` and `super::` path in `text`, as the offset it starts
/// at and the paths it names, from the crate root and with groups spelled
/// out: `crate::{Tool, tool::util}` is `Tool` and `tool::util`.
///
/// A tool lives in `src/tool/`, so in its file `super` is `crate::tool`
/// and `super::super` is the crate root; a path that climbs higher has no
/// root, and is kept as one nobody vetted. (`super` inside an inline
/// module is read the same way, which can only report more.)
fn module_paths(text: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut found: Vec<(usize, &str)> = Vec::new();
    for (at, _) in text.match_indices("crate::") {
        if !text[..at].chars().next_back().is_some_and(is_ident_char) {
            found.push((at, "crate::"));
        }
    }
    for (at, _) in text.match_indices("super::") {
        // The start of a chain: not the middle of `super::super::`, and
        // not the end of a longer word. After `self::` is fine.
        let before = &text[..at];
        if !before.ends_with("super::") && !before.chars().next_back().is_some_and(is_ident_char) {
            found.push((at, "super::"));
        }
    }
    for (at, keyword) in found {
        let mut rest = &text[at + keyword.len()..];
        let mut prefix = String::new();
        if keyword == "super::" {
            let mut climbed = 1;
            while let Some(more) = rest.strip_prefix("super::") {
                climbed += 1;
                rest = more;
            }
            prefix = match climbed {
                1 => "tool::".to_string(),
                2 => String::new(),
                _ => "above-the-crate-root::".to_string(),
            };
        }
        // The path, and a group after it if it ends in `::` (or is empty).
        let mut end = rest
            .find(|c: char| !(is_ident_char(c) || c == ':'))
            .unwrap_or(rest.len());
        if rest[end..].starts_with('{') && (end == 0 || rest[..end].ends_with("::")) {
            end = closing_brace(rest, end).map_or(rest.len(), |close| close + 1);
        }
        for path in expand_tree(&rest[..end]) {
            out.push((at, format!("{prefix}{path}")));
        }
        // `crate::*` and `super::*` name no path: the glob is the item.
        if rest[..end].is_empty() {
            out.push((at, format!("{prefix}*")));
        }
    }
    out.sort();
    out
}

/// Whether a path from the crate root stays inside [`REACHABLE`] and
/// [`ROOT_ITEMS`].
fn reachable(path: &str) -> bool {
    // An item at the crate root, with or without a path below it
    // (`ToolResult::Error`).
    let first = path.split("::").next().unwrap_or_default();
    ROOT_ITEMS.contains(&first)
        || REACHABLE
            .iter()
            .any(|ok| path == *ok || path.starts_with(&format!("{ok}::")))
}

/// Where each `type` item that names the store starts. A `type` item is an
/// alias, so `type S = session::Store;` hides `S::open(p)`, which has no
/// `Store::open` in it, from the token scan.
fn store_aliases(text: &str) -> Vec<usize> {
    let mut out = Vec::new();
    for (at, _) in text.match_indices("type") {
        let before = text[..at].chars().next_back();
        let after = text[at + 4..].chars().next();
        // The keyword: not `subtype`, `type_name` or `r#type`.
        if before.is_some_and(|c| is_ident_char(c) || c == '#')
            || !after.is_some_and(char::is_whitespace)
        {
            continue;
        }
        // Up to the `;` that ends the item, not one inside `[T; 4]`.
        let mut depth = 0;
        let mut end = text.len();
        for (i, c) in text[at..].char_indices() {
            match c {
                '(' | '[' | '{' => depth += 1,
                ')' | ']' | '}' => depth -= 1,
                ';' if depth <= 0 => {
                    end = at + i;
                    break;
                }
                _ => {}
            }
        }
        let item = &text[at..end];
        let names_store = item.match_indices("Store").any(|(i, _)| {
            let word_before = item[..i].chars().next_back();
            let word_after = item[i + "Store".len()..].chars().next();
            !word_before.is_some_and(is_ident_char) && !word_after.is_some_and(is_ident_char)
        });
        if names_store {
            out.push(at);
        }
    }
    out
}

fn offences_in(file: &str, source: &str) -> Vec<String> {
    let lines = code_of(source);
    let mut found: Vec<(usize, String)> = Vec::new();
    for (n, line) in &lines {
        for &(token, why) in FORBIDDEN {
            if line.contains(token) {
                found.push((
                    *n,
                    format!("{file}:{n}: `{token}` ({why}): {}", line.trim()),
                ));
            }
        }
    }

    // What spans lines: paths, and the items that alias the store.
    let code = Code::new(&lines);
    for (at, path) in module_paths(&code.text) {
        if !reachable(&path) {
            let (n, line) = code.locate(at);
            found.push((
                n,
                format!(
                    "{file}:{n}: `{path}` (from the crate root) is a module this scan has not been \
                     taught to trust (REACHABLE): {line}"
                ),
            ));
        }
    }
    for at in store_aliases(&code.text) {
        let (n, line) = code.locate(at);
        found.push((
            n,
            format!(
                "{file}:{n}: `type` (an alias for the store hides `Store::open` from this \
                 scan): {line}"
            ),
        ));
    }

    // In the order of the file, whichever pass found them.
    found.sort_by_key(|(n, _)| *n);
    found.into_iter().map(|(_, text)| text).collect()
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
///
/// What it reads is text, so it does not guard against a tool written to
/// get past it. The spellings that need no such effort are found and
/// tested (`the_scan_finds_the_spellings_that_hide_a_second_store`): an
/// alias (`use ... as`, `type S = Store;`), a qualified path
/// (`<Store>::open`), a rename in a `use` group, `super::` paths, and a
/// `use crate::{...}` that rustfmt wraps onto several lines. What it does
/// not see, because telling them from ordinary code needs the compiler:
/// whitespace or a line break inside a path (`Store :: open`), and a path
/// put together by a macro (`paste!`). Reaching a second store through
/// either is a decision someone made, and review is the guard for it.
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

/// The line of each offence the scan finds in `source`, one entry per
/// offence.
fn found(source: &str) -> Vec<usize> {
    offences_in("fixture.rs", source)
        .iter()
        .map(|o| {
            o.split(':')
                .nth(1)
                .and_then(|n| n.parse().ok())
                .expect("a line number")
        })
        .collect()
}

/// The scan only means something if it can fail.
#[test]
fn the_scan_finds_what_it_looks_for() {
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

/// The scan reads text, so a second `Store` reached by a spelling it does
/// not search for gets through. These are the spellings that take no
/// effort to write, and one that rustfmt produces on its own.
#[test]
fn the_scan_finds_the_spellings_that_hide_a_second_store() {
    // An alias: `S::open` never contains `Store::open`. The line is the
    // alias's, wherever its type is written.
    assert_eq!(
        found("type S = session::Store;\nfn f() { S::open(p); }\n"),
        vec![1]
    );
    assert_eq!(found("pub type S =\n    session::Store;\n"), vec![1]);
    assert_eq!(found("type S = Box<Store>;\n"), vec![1]);
    // A qualified path, and a rename inside a `use` group.
    assert_eq!(found("fn f() { <session::Store>::open(p); }\n"), vec![1]);
    assert_eq!(found("fn f() { <Store>::open(p); }\n"), vec![1]);
    assert_eq!(found("use session::Store::{self as S};\n"), vec![1]);
    // What is not an alias for the store: other types, associated types,
    // a function that takes a store, and `type` inside a longer word.
    assert!(found(
        "type Output = Foo;\ntrait T { type Item; }\nimpl T for U { type Item = u8; }\n\
         fn f(s: &session::Store, type_name: &str) { let subtype = 1; }\n"
    )
    .is_empty());
    // A comment or a string may say anything.
    assert!(found("// type S = Store;\nlet a = \"type S = Store;\";\n").is_empty());

    // `super` in a tool's file is `crate::tool`, and `super::super` is the
    // crate root: the same paths as `crate::`, held to the same list.
    assert!(found(
        "use super::util::edit;\nuse super::{util, util::edit};\n\
         use super::super::schema::anthropic_tool;\nuse super::super::{Tool, ToolResult};\n"
    )
    .is_empty());
    assert_eq!(found("use super::batch::run;\n"), vec![1]);
    assert_eq!(found("fn f() { super::batch::run(x); }\n"), vec![1]);
    assert_eq!(found("use super::super::batch::run;\n"), vec![1]);
    assert_eq!(found("use self::super::batch::run;\n"), vec![1]);
    assert_eq!(found("use super::{util, batch};\n"), vec![1]);
    assert_eq!(found("use super::*;\n"), vec![1]);
    assert_eq!(
        found("use super::super::super::batch::run;\n"),
        vec![1],
        "above the crate root is not a path anyone vetted"
    );
    assert!(found("fn f() { my_super::batch::run(x); let unsuper = 1; }\n").is_empty());

    // A group that rustfmt wraps. Its items are on lines of their own, and
    // each is checked as a path of its own.
    assert!(found("use crate::{\n    Tool,\n    ToolContext,\n};\n").is_empty());
    assert_eq!(
        found("use crate::{\n    Tool,\n    batch::run,\n};\n"),
        vec![1]
    );
    assert_eq!(found("use super::{\n    util,\n    batch,\n};\n"), vec![1]);
    assert!(found("use crate::{tool::util::edit, Tool};\n").is_empty());
}

/// Integer arguments of tools that run off the lock that are **not** tracks,
/// as `(tool, argument, what it is)`.
///
/// The re-run guard reads a track from an argument whose name contains
/// `track` and nothing else, so an integer argument with any other name is
/// either not a track, and is listed here with what it is, or is a track in
/// a place the guard does not look. Keyed by tool as well as by argument,
/// so a second tool that adds a `times` is read again, not waved through.
const NOT_TRACKS: &[(&str, &str, &str)] = &[
    (
        "phaser",
        "stages",
        "the number of all-pass stages in the filter chain (2-12)",
    ),
    (
        "repeat_selection",
        "times",
        "how many extra copies of the selection to append",
    ),
    (
        "resample_track",
        "target_sample_rate",
        "a sample rate in Hz (the track to resample is `track`)",
    ),
];

/// A property found in a tool's `input_schema`, at any depth.
struct Prop {
    /// Where it sits, `segments[].track`.
    path: String,
    /// Its own name, `track`.
    name: String,
    /// How many `properties` maps were crossed to reach it. 1 is an
    /// argument of the call itself, which is all `addressed_tracks` reads.
    depth: usize,
    spec: Value,
}

/// Everything under a schema keyword that holds a schema (`items`, `oneOf`,
/// `additionalProperties`, ...) is walked; the keywords that hold values,
/// not schemas, are not.
const VALUE_KEYWORDS: &[&str] = &["enum", "const", "default", "examples", "required"];

/// Every property in `node`, nested ones too, and the `$ref`s, which this
/// guard does not follow and so cannot read through.
fn walk_schema(
    node: &Value,
    path: &str,
    depth: usize,
    props: &mut Vec<Prop>,
    refs: &mut Vec<String>,
) {
    match node {
        Value::Array(schemas) => {
            for schema in schemas {
                walk_schema(schema, path, depth, props, refs);
            }
        }
        Value::Object(map) => {
            for (key, value) in map {
                if key == "properties" {
                    for (name, spec) in value.as_object().into_iter().flatten() {
                        let at = if path.is_empty() {
                            name.clone()
                        } else {
                            format!("{path}.{name}")
                        };
                        props.push(Prop {
                            path: at.clone(),
                            name: name.clone(),
                            depth: depth + 1,
                            spec: spec.clone(),
                        });
                        walk_schema(spec, &at, depth + 1, props, refs);
                    }
                } else if key == "$ref" {
                    refs.push(path.to_string());
                } else if !VALUE_KEYWORDS.contains(&key.as_str()) {
                    let below = if key == "items" {
                        format!("{path}[]")
                    } else {
                        path.to_string()
                    };
                    walk_schema(value, &below, depth, props, refs);
                }
            }
        }
        _ => {}
    }
}

/// Whether `spec`'s `type` is `want`, or lists it.
fn has_type(spec: &Value, want: &str) -> bool {
    match &spec["type"] {
        Value::String(t) => t == want,
        Value::Array(ts) => ts.iter().any(|t| t == want),
        _ => false,
    }
}

/// Whether any of `schemas` (one schema, or a list of them) satisfies `f`.
fn any_schema(schemas: &Value, f: fn(&Value) -> bool) -> bool {
    match schemas {
        Value::Array(list) => list.iter().any(f),
        one => f(one),
    }
}

/// Whether an argument can carry a whole number: it is one, or a list of
/// them, or one of several shapes is.
fn holds_integers(spec: &Value) -> bool {
    has_type(spec, "integer")
        || spec
            .get("items")
            .is_some_and(|i| any_schema(i, holds_integers))
        || ["anyOf", "oneOf", "allOf"]
            .iter()
            .any(|k| spec.get(*k).is_some_and(|s| any_schema(s, holds_integers)))
}

/// A whole number or a list of them: the two shapes `addressed_tracks`
/// reads.
fn reads_as_positions(spec: &Value) -> bool {
    has_type(spec, "integer")
        || (has_type(spec, "array") && spec.get("items").is_some_and(|i| has_type(i, "integer")))
}

/// What is wrong, for the re-run guard, with the way `tool` takes its
/// arguments. `input_schema` is the tool's `input_schema`.
///
/// 1. An argument with `track` in its name must be a whole number or a list
///    of them (what `addressed_tracks` reads), and must be an argument of
///    the call itself: `addressed_tracks` does not look inside an object or
///    a list of objects, so `segments: [{track}]` would be re-run on a
///    session it no longer describes, with no error.
/// 2. An integer argument with any other name must be listed in
///    [`NOT_TRACKS`], with what it is. Otherwise a track called `source` or
///    `index` would be invisible to the guard in the same way. This holds
///    at any depth.
/// 3. An argument with no `type`, or a `$ref`, cannot be read at all.
fn track_problems(tool: &str, input_schema: &Value) -> Vec<String> {
    let (mut props, mut refs) = (Vec::new(), Vec::new());
    walk_schema(input_schema, "", 0, &mut props, &mut refs);

    let mut problems: Vec<String> = refs
        .iter()
        .map(|at| {
            format!("{tool}: `$ref` at `{at}`, which this guard does not follow, so it cannot tell whether a track is behind it")
        })
        .collect();
    for Prop {
        path,
        name,
        depth,
        spec,
    } in &props
    {
        if name.contains("track") {
            if *depth != 1 {
                problems.push(format!(
                    "{tool}: `{path}` names a track inside another argument, where \
                     `addressed_tracks` does not look"
                ));
            }
            if !reads_as_positions(spec) {
                problems.push(format!(
                    "{tool}: `{path}` is {spec}, which `addressed_tracks` does not read"
                ));
            }
        } else if holds_integers(spec)
            && !NOT_TRACKS
                .iter()
                .any(|(t, a, _)| (*t, *a) == (tool, path.as_str()))
        {
            problems.push(format!(
                "{tool}: `{path}` is a whole number whose name does not contain `track`. If it \
                 is a track, rename it; if not, add (\"{tool}\", \"{path}\") to NOT_TRACKS with \
                 what it is"
            ));
        }
        let combinator = ["$ref", "anyOf", "oneOf", "allOf"]
            .iter()
            .any(|k| spec.get(*k).is_some());
        if spec.get("type").is_none() && !combinator {
            problems.push(format!(
                "{tool}: `{path}` has no `type`, so the guard cannot tell whether it holds a track"
            ));
        }
    }
    problems
}

/// The guard in `Shared::dispatch` (a re-run must mean what the first run
/// meant) finds the tracks a call names by reading its arguments:
/// `tools::shared::addressed_tracks` takes every top-level argument with
/// `track` in its name that holds a whole number or a list of them. A tool
/// that names a track some other way — an object, a string, a list of
/// objects, an integer with another name — would be re-run on a session its
/// arguments no longer describe, with no error, so every tool that runs off
/// the lock is checked, at every depth of its schema, to name its tracks
/// the way the guard reads them (see [`track_problems`]).
#[test]
fn every_off_lock_tool_names_its_tracks_where_the_dispatch_guard_looks() {
    let dispatcher = ToolDispatcher::default_dispatcher();
    let mut problems = Vec::new();
    for name in OFF_LOCK {
        let schema = dispatcher.get(name).expect("registered").schema();
        problems.extend(track_problems(name, &schema["input_schema"]));
    }
    assert!(
        problems.is_empty(),
        "an off-lock tool names a track in a way the re-run guard cannot see:\n  {}\n\n\
         Name it with an integer (or a list of integers) in a top-level argument whose name \
         contains `track`, or teach `tools::shared::addressed_tracks` the new shape.",
        problems.join("\n  ")
    );

    // And the tools that take a track at all do: a vacuous pass would be
    // an empty list of tools or of arguments.
    let top_level = |name: &str| -> Vec<String> {
        let schema = dispatcher.get(name).unwrap().schema();
        let (mut props, mut refs) = (Vec::new(), Vec::new());
        walk_schema(&schema["input_schema"], "", 0, &mut props, &mut refs);
        props
            .into_iter()
            .filter(|p| p.depth == 1)
            .map(|p| p.name)
            .collect()
    };
    let named: Vec<&str> = OFF_LOCK
        .iter()
        .copied()
        .filter(|name| top_level(name).iter().any(|k| k.contains("track")))
        .collect();
    assert!(
        named.len() >= 35 && named.contains(&"time_stretch") && named.contains(&"mix_to_new_track"),
        "{named:?}"
    );

    // Every allowlisted argument is still there, on a tool that is still
    // off the lock, and still a whole number: an entry that matches nothing
    // would wave a future property of that name through.
    for (tool, arg, what) in NOT_TRACKS {
        assert!(
            OFF_LOCK.contains(tool),
            "{tool} is not off the lock ({what})"
        );
        let schema = dispatcher.get(tool).expect("registered").schema();
        let (mut props, mut refs) = (Vec::new(), Vec::new());
        walk_schema(&schema["input_schema"], "", 0, &mut props, &mut refs);
        assert!(
            props
                .iter()
                .any(|p| p.path == *arg && holds_integers(&p.spec)),
            "NOT_TRACKS lists {tool}.{arg} ({what}), which is not a whole number of that tool"
        );
    }
}

/// The track guard only means something if it can fail. These are the
/// shapes it exists for, as schemas of their own so that no real tool has to
/// carry one.
#[test]
fn the_track_guard_finds_what_it_looks_for() {
    use serde_json::json;
    let problems_of = |tool: &str, schema: Value| track_problems(tool, &schema);

    // What the real tools do passes: a whole number, a list of them, under
    // any name that has `track` in it, at the top; other arguments that are
    // not whole numbers.
    assert!(problems_of(
        "echo",
        json!({ "type": "object", "properties": {
            "track": { "type": "integer" },
            "source_track": { "type": "integer" },
            "track_indices": { "type": "array", "items": { "type": "integer" } },
            "gain_db": { "type": "number" },
            "kind": { "type": "string" },
            "range": { "type": "object", "properties": {
                "start_sec": { "type": "number" } } },
        } })
    )
    .is_empty());

    // The reviewer's case: a track inside a list of objects, and inside an
    // object. Both used to pass.
    let nested = problems_of(
        "echo",
        json!({ "type": "object", "properties": {
            "track": { "type": "integer" },
            "segments": { "type": "array", "items": { "type": "object", "properties": {
                "track": { "type": "integer" } } } },
            "region": { "type": "object", "properties": {
                "source_track": { "type": "integer" } } },
        } }),
    );
    assert_eq!(nested.len(), 2, "{nested:?}");
    for at in ["segments[].track", "region.source_track"] {
        assert!(nested.iter().any(|p| p.contains(at)), "{at}: {nested:?}");
    }

    // Nested under a combinator, too.
    let combined = problems_of(
        "echo",
        json!({ "type": "object", "properties": {
            "where": { "anyOf": [{ "type": "object", "properties": {
                "track": { "type": "integer" } } }] },
        } }),
    );
    assert_eq!(combined.len(), 1, "{combined:?}");

    // Wrong type at the top: reported as before.
    assert_eq!(
        problems_of(
            "echo",
            json!({ "properties": { "track": { "type": "string" } } })
        )
        .len(),
        1
    );
    assert_eq!(
        problems_of(
            "echo",
            json!({ "properties": { "track_name": { "type": "string" } } })
        )
        .len(),
        1
    );

    // A whole number that is not named for a track: top level, nested, in a
    // list, and in a union.
    for spec in [
        json!({ "type": "integer" }),
        json!({ "type": ["integer", "null"] }),
        json!({ "type": "array", "items": { "type": "integer" } }),
        json!({ "anyOf": [{ "type": "integer" }, { "type": "null" }] }),
    ] {
        let sneaky = problems_of("echo", json!({ "properties": { "source": spec.clone() } }));
        assert_eq!(sneaky.len(), 1, "{spec}: {sneaky:?}");
    }
    let deep = problems_of(
        "echo",
        json!({ "properties": { "segments": { "type": "array", "items": {
            "type": "object", "properties": { "index": { "type": "integer" } } } } } }),
    );
    assert_eq!(deep.len(), 1, "{deep:?}");
    assert!(deep[0].contains("segments[].index"), "{deep:?}");

    // The allowlist is per tool: `stages` is the phaser's, not everyone's,
    // and only at the top.
    let stages = json!({ "properties": { "stages": { "type": "integer" } } });
    assert!(problems_of("phaser", stages.clone()).is_empty());
    assert_eq!(problems_of("echo", stages).len(), 1);
    let nested_stages = json!({ "properties": { "extra": { "type": "object", "properties": {
        "stages": { "type": "integer" } } } } });
    assert_eq!(problems_of("phaser", nested_stages).len(), 1);

    // What cannot be read: no type, a reference.
    assert_eq!(
        problems_of("echo", json!({ "properties": { "source": {} } })).len(),
        1
    );
    assert_eq!(
        problems_of(
            "echo",
            json!({ "properties": { "source": { "$ref": "#/definitions/T" } } })
        )
        .len(),
        1,
        "the reference itself; the definition behind it is not read"
    );
}
