//! No command that waits on a lock a running tool holds runs on the main
//! thread (#421).
//!
//! Tauri 2 runs a command declared without `async` on the main thread,
//! and the agent loop holds the store, engine, dispatcher and clipboard
//! locks for the whole of a tool call. A main-thread command that waits on
//! one of them freezes the window for as long as the tool runs: no
//! repaint, no input, no transport. A time-stretch of a 32 s track froze it
//! for about ten seconds.
//!
//! The lock can be reached through a helper as easily as directly:
//! twelve synchronous commands — the mixer's faders and mutes, renames,
//! clip moves — took it inside `run_track_tool`, so a scan of command
//! bodies alone passed while a fader click during a long tool still froze
//! the window. The scan therefore follows calls into the crate's free
//! functions, as far as they go.
//!
//! The source is scanned rather than the behaviour exercised because the
//! mock runtime does not model Tauri's threads.

use std::collections::BTreeSet;
use std::path::PathBuf;

/// What a running agent turn can hold for the length of a tool: the
/// session store, the engine, the dispatcher and the clipboard. Waiting on
/// any of them from the main thread freezes the window.
const TURN_LOCKS: [&str; 7] = [
    "store_handle()",
    "lock_std(&*store",
    "lock_std(&store",
    "lock_std(&state.engine",
    "lock_std(&state.dispatcher",
    "state.dispatcher.lock()",
    "clipboard_handle()",
];

fn takes_a_turn_lock(body: &str) -> bool {
    TURN_LOCKS.iter().any(|t| body.contains(t))
}

/// Whether `body` calls the free function `name`: `name(` not preceded by
/// an identifier character or a `.`, which would make it another name or a
/// method.
fn calls(body: &str, name: &str) -> bool {
    let needle = format!("{name}(");
    let mut from = 0;
    while let Some(i) = body[from..].find(&needle) {
        let at = from + i;
        let before = body[..at].chars().next_back();
        let is_part_of_another =
            before.is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '.');
        let is_definition = body[..at].ends_with("fn ");
        if !is_part_of_another && !is_definition {
            return true;
        }
        from = at + needle.len();
    }
    false
}

/// The source of every `.rs` file in `src`, with Windows line endings
/// normalised: the scans split on "\n".
fn sources() -> Vec<(String, String)> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&dir).expect("src") {
        let path = entry.expect("entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let src = std::fs::read_to_string(&path)
            .expect("read")
            .replace("\r\n", "\n");
        out.push((
            path.file_name().unwrap().to_string_lossy().into_owned(),
            src,
        ));
    }
    out
}

/// Every free function at column 0 in `src`, as (name, body). The body
/// runs to the first closing brace at column 0, as for commands.
fn helpers() -> Vec<(String, String)> {
    let mut out = Vec::new();
    for (_, src) in sources() {
        let mut offset = 0;
        for line in src.split_inclusive('\n') {
            let start = offset;
            offset += line.len();
            let sig = [
                "fn ",
                "pub fn ",
                "pub(crate) fn ",
                "async fn ",
                "pub async fn ",
                "pub(crate) async fn ",
            ]
            .iter()
            .find_map(|p| line.strip_prefix(p));
            let Some(sig) = sig else { continue };
            let name: String = sig
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            let rest = &src[start..];
            let end = rest.find("\n}\n").map(|i| i + 2).unwrap_or(rest.len());
            out.push((name, rest[..end].to_string()));
        }
    }
    out
}

/// Names of the free functions that take a turn lock, directly or through
/// another one: grown until nothing new joins.
fn lock_taking_helpers() -> BTreeSet<String> {
    let helpers = helpers();
    let mut set: BTreeSet<String> = helpers
        .iter()
        .filter(|(_, body)| takes_a_turn_lock(body))
        .map(|(name, _)| name.clone())
        .collect();
    loop {
        let before = set.len();
        for (name, body) in &helpers {
            if !set.contains(name) && set.iter().any(|h| calls(body, h)) {
                set.insert(name.clone());
            }
        }
        if set.len() == before {
            return set;
        }
    }
}

/// Every `#[tauri::command]` in `src`, as (file, name, attribute, is_async_fn, body).
fn commands() -> Vec<(String, String, String, bool, String)> {
    let mut out = Vec::new();
    for (file, src) in sources() {
        let mut rest = src.as_str();
        while let Some(at) = rest.find("#[tauri::command") {
            let after = &rest[at..];
            let attr_end = after.find(']').expect("attribute end");
            let attr = after[..=attr_end].to_string();
            let sig_at = after.find("pub ").expect("command signature");
            let sig = &after[sig_at..];
            let is_async = sig.starts_with("pub async fn");
            let name_start = sig.find("fn ").expect("fn") + 3;
            let name: String = sig[name_start..]
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            // The body runs to the first closing brace at column 0.
            let body_end = sig.find("\n}\n").map(|i| i + 2).unwrap_or(sig.len());
            out.push((
                file.clone(),
                name,
                attr,
                is_async,
                sig[..body_end].to_string(),
            ));
            rest = &after[sig_at + body_end.max(1)..];
        }
    }
    out
}

#[test]
fn no_main_thread_command_waits_on_a_lock_a_tool_holds() {
    let all = commands();
    assert!(
        all.len() > 50,
        "found only {} commands; the scan is broken",
        all.len()
    );
    let helpers = lock_taking_helpers();
    assert!(
        helpers.contains("run_track_tool"),
        "the helper scan no longer finds `run_track_tool`, which locks the store; \
         the scan is broken: {helpers:?}"
    );
    let offenders: Vec<String> = all
        .iter()
        .filter(|(_, _, attr, is_async, _)| !is_async && !attr.contains("async"))
        .filter_map(|(file, name, _, _, body)| {
            if takes_a_turn_lock(body) {
                return Some(format!("{file}: {name}"));
            }
            helpers
                .iter()
                .find(|h| calls(body, h))
                .map(|h| format!("{file}: {name} (through `{h}`)"))
        })
        .collect();
    assert!(
        offenders.is_empty(),
        "these commands run on the main thread and wait on a lock a running \
         tool holds, freezing the window: {offenders:?}. Declare them \
         `#[tauri::command(async)]` or `pub async fn`."
    );
}

/// The call check is what the helper scan rests on, so it is checked
/// against the shapes it must tell apart.
#[test]
fn the_call_check_tells_a_call_from_a_lookalike() {
    assert!(calls(
        "    run_track_tool(&state, \"gain\", args)",
        "run_track_tool"
    ));
    assert!(calls(
        "crate::commands::run_track_tool(&state)",
        "run_track_tool"
    ));
    assert!(calls("let x = (run_track_tool(&state));", "run_track_tool"));
    assert!(!calls("    my_run_track_tool(&state)", "run_track_tool"));
    assert!(!calls("    state.run_track_tool(x)", "run_track_tool"));
    assert!(!calls(
        "fn run_track_tool(state: &AppState)",
        "run_track_tool"
    ));
}
