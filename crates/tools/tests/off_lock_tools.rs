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
const FORBIDDEN: [(&str, &str); 15] = [
    (
        "clipboard",
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
    ("ctx.engine", "the engine is a fresh one in an off-lock run"),
    (
        "Store::open",
        "a second handle on the store bypasses staging",
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

/// The code of a source file, without its tests or comment lines.
fn code_of(source: &str) -> Vec<(usize, &str)> {
    let end = source.find("#[cfg(test)]").unwrap_or(source.len());
    source[..end]
        .lines()
        .enumerate()
        .map(|(i, line)| (i + 1, line))
        .filter(|(_, line)| !line.trim_start().starts_with("//"))
        .collect()
}

fn offences(file: &str) -> Vec<String> {
    let source = source_of(file);
    let mut out = Vec::new();
    for (n, line) in code_of(&source) {
        for (token, why) in FORBIDDEN {
            if line.contains(token) {
                out.push(format!("{file}:{n}: `{token}` ({why}): {}", line.trim()));
            }
        }
    }
    out
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
    let source = "fn f() {\n    // ctx.clipboard in a comment\n    ctx.store.set_head(x);\n}\n\
                  #[cfg(test)]\nmod tests { fn g() { ctx.engine; } }\n";
    let found: Vec<_> = code_of(source)
        .into_iter()
        .filter(|(_, l)| FORBIDDEN.iter().any(|(t, _)| l.contains(t)))
        .collect();
    assert_eq!(found, vec![(3, "    ctx.store.set_head(x);")]);
}
