//! No command that takes the session store's lock runs on the main
//! thread (#421).
//!
//! Tauri 2 runs a command declared without `async` on the main thread,
//! and the agent loop holds the store lock for the whole of a tool call.
//! A main-thread command that waits on that lock freezes the window for
//! as long as the tool runs: no repaint, no input, no transport. A
//! time-stretch of a 32 s track froze it for about ten seconds.
//!
//! The source is scanned rather than the behaviour exercised because the
//! mock runtime does not model Tauri's threads.

use std::path::PathBuf;

/// Every `#[tauri::command]` in `src`, as (file, name, attribute, is_async_fn, body).
fn commands() -> Vec<(String, String, String, bool, String)> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&dir).expect("src") {
        let path = entry.expect("entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let src = std::fs::read_to_string(&path).expect("read");
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
                path.file_name().unwrap().to_string_lossy().into_owned(),
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
fn no_main_thread_command_takes_the_store_lock() {
    let all = commands();
    assert!(all.len() > 50, "found only {} commands; the scan is broken", all.len());
    let offenders: Vec<String> = all
        .iter()
        .filter(|(_, _, attr, is_async, _)| !is_async && !attr.contains("async"))
        .filter(|(_, _, _, _, body)| body.contains("store_handle()") || body.contains("lock_std(&*store") || body.contains("lock_std(&store"))
        .map(|(file, name, ..)| format!("{file}: {name}"))
        .collect();
    assert!(
        offenders.is_empty(),
        "these commands run on the main thread and wait on the store lock a running \
         tool holds, freezing the window: {offenders:?}. Declare them \
         `#[tauri::command(async)]` or `pub async fn`."
    );
}
