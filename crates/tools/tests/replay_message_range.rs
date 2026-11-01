//! An edit whose range came from the chat message replays (#377, row 3
//! of #356).
//!
//! With a selection, the app prefixes the message with `[apply to …]`,
//! and `fade`, `reverse` and `copy_region` read their range from it when
//! the call names none. The op recorded only the call's own arguments, so
//! a replay — which has no message — faded nothing (`range is required`)
//! or reversed the whole track, and the history sweep kept that audio for
//! good. The dispatcher now records the range the call used.

use std::path::PathBuf;

use serde_json::{json, Value};
use tools::ToolResult;

#[path = "support/harness.rs"]
mod harness;
use harness::{write_frames, Session};

const SR: u32 = 48_000;
const SELECTION: &str = "[apply to 0:01-0:02] ";

impl Session {
    fn new() -> Self {
        let mut s = Session::empty();
        let take = write_frames(
            &s.dir.path().join("take.wav"),
            SR,
            1,
            SR as usize * 4,
            |n| {
                let t = n as f32 / SR as f32;
                // A rising tone, so a reversed span differs from the original.
                (2.0 * std::f32::consts::PI * (220.0 + 110.0 * t) * t).sin() * 8000.0 / 32_767.0
            },
        );
        s.load(&take);
        s
    }

    /// Run `tool` from a chat message and take its value.
    fn say(&mut self, tool: &str, args: Value, message: &str) -> Value {
        match self.call_saying(tool, args, message) {
            ToolResult::Ok(v) => v,
            ToolResult::Error(m) => panic!("{tool} failed: {m}"),
        }
    }

    fn head_audio(&self) -> PathBuf {
        self.store.get(self.head()).expect("node").state.tracks[0].clips[0]
            .source_path
            .clone()
    }
}

/// Run `tool` with its range only in the message, make it history, sweep
/// everything, and check its audio went and comes back byte for byte.
fn swept_and_back(tool: &str) {
    let mut s = Session::new();
    s.say(tool, json!({ "track": 0 }), &format!("{SELECTION}do it"));
    let node = s.head();
    let audio = s.head_audio();
    let bytes = std::fs::read(&audio).expect("edited audio");
    s.say(
        "silence_region",
        json!({ "track": 0, "start_sec": 3.0, "end_sec": 3.2 }),
        "",
    );

    let report = tools::reclaim::sweep(&s.store, 0).expect("sweep");

    assert_eq!(report.kept_unverified, 0, "{report:?}");
    assert!(!audio.is_file(), "the {tool} audio was swept: {report:?}");
    tools::rederive::materialize(&s.store, node).expect("rebuilt");
    assert!(
        std::fs::read(&audio).expect("back") == bytes,
        "and came back byte for byte"
    );
}

#[test]
fn a_fade_over_the_selection_is_swept_and_comes_back() {
    swept_and_back("fade");
}

/// `reverse` without a range reverses the whole track, so a replay that
/// lost the selection did not fail — it made different audio.
#[test]
fn a_reverse_of_the_selection_is_swept_and_comes_back() {
    swept_and_back("reverse");
}

#[test]
fn the_op_records_the_range_the_call_used() {
    let mut s = Session::new();
    s.say(
        "fade",
        json!({ "track": 0 }),
        &format!("{SELECTION}fade it"),
    );

    let op = s.store.get(s.head()).expect("node").op.expect("op");
    assert_eq!(
        op.params["range"],
        json!({ "start_sec": 1.0, "end_sec": 2.0 })
    );
}

/// A range the call named itself is what it used, whatever the message
/// says — and is recorded as given.
#[test]
fn a_range_the_call_named_is_recorded_as_given() {
    let mut s = Session::new();
    let named = json!({ "start_sec": 0.25, "end_sec": 0.75 });
    s.say(
        "fade",
        json!({ "track": 0, "range": named }),
        &format!("{SELECTION}fade it"),
    );

    let op = s.store.get(s.head()).expect("node").op.expect("op");
    assert_eq!(op.params["range"], named);
}

/// A tool that does not read the message must not be given its range:
/// its replay would then use a range it never did.
#[test]
fn a_tool_that_ignores_the_message_records_no_range_from_it() {
    let mut s = Session::new();
    s.say(
        "silence_region",
        json!({ "track": 0, "start_sec": 0.1, "end_sec": 0.2 }),
        &format!("{SELECTION}silence"),
    );

    let op = s.store.get(s.head()).expect("node").op.expect("op");
    assert!(op.params.get("range").is_none(), "{}", op.params);
}

/// Every tool that reads the message is listed, so the dispatcher records
/// its range; and every listed tool is real and does read it.
#[test]
fn every_tool_that_reads_the_message_is_listed() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/tool");
    let mut reading = Vec::new();
    for entry in std::fs::read_dir(&dir).expect("src/tool") {
        let path = entry.expect("entry").path();
        let src = std::fs::read_to_string(&path).unwrap_or_default();
        if src.contains("ctx.user_message") {
            reading.push(path.file_stem().unwrap().to_string_lossy().to_string());
        }
    }
    reading.sort();
    let mut listed: Vec<String> = tools::READS_RANGE_FROM_MESSAGE
        .iter()
        .map(|s| s.to_string())
        .collect();
    listed.sort();

    assert!(reading.len() >= 3, "found {reading:?} — the layout moved");
    assert_eq!(
        reading, listed,
        "a tool reads the chat message but the dispatcher does not record \
         the range it took from it, so it cannot replay"
    );
}
