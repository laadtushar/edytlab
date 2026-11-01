//! Pan edits through the dispatcher and the tool's schema-bypassing boundary.

use std::path::{Path, PathBuf};

use hound::{SampleFormat, WavSpec, WavWriter};
use serde_json::{json, Value};
use tempfile::TempDir;
use tools::tool::SetPanTool;
use tools::{DispatchError, Tool, ToolContext, ToolDispatcher, ToolResult};

const SAMPLE_RATE: u32 = 48_000;
const TRACK_COUNT: usize = 2;
const FRAMES: usize = 480;

fn write_wav(dir: &Path, name: &str, frequency: f32) -> PathBuf {
    let path = dir.join(name);
    let spec = WavSpec {
        channels: 1,
        sample_rate: SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut writer = WavWriter::create(&path, spec).expect("create WAV");
    for frame in 0..FRAMES {
        let phase = 2.0 * std::f32::consts::PI * frequency * frame as f32 / SAMPLE_RATE as f32;
        writer
            .write_sample((phase.sin() * 16_000.0) as i16)
            .expect("write WAV sample");
    }
    writer.finalize().expect("finish WAV");
    path
}

struct Session {
    _dir: TempDir,
    store: session::Store,
    engine: audio_engine::Engine,
    dispatcher: ToolDispatcher,
    clipboard: Option<tools::Clipboard>,
}

impl Session {
    fn with_two_tracks(selected_track: usize) -> Self {
        let dir = TempDir::new().expect("tempdir");
        let first = write_wav(dir.path(), "first.wav", 220.0);
        let second = write_wav(dir.path(), "second.wav", 330.0);
        let store = session::Store::open(dir.path()).expect("open store");
        let mut session = Self {
            _dir: dir,
            store,
            engine: audio_engine::Engine::new(),
            dispatcher: ToolDispatcher::default_dispatcher(),
            clipboard: None,
        };
        expect_ok(session.call("load", json!({ "path": first.to_string_lossy() })));
        expect_ok(session.call("load", json!({ "path": second.to_string_lossy() })));
        assert_eq!(session.state().tracks.len(), TRACK_COUNT);

        let other_track = 1 - selected_track;
        expect_ok(session.call("set_pan", json!({ "track": other_track, "pan": 0.75 })));
        assert_eq!(session.state().tracks[other_track].pan, 0.75);
        assert_eq!(session.state().tracks[selected_track].pan, 0.0);
        session
    }

    fn call(&mut self, tool: &str, args: Value) -> tools::Result<ToolResult> {
        let mut context = ToolContext {
            store: &mut self.store,
            engine: &mut self.engine,
            user_message: "",
            clipboard: &mut self.clipboard,
            allowed_tools: None,
        };
        self.dispatcher.invoke(tool, args, &mut context)
    }

    fn call_tool_directly(&mut self, args: Value) -> tools::Result<ToolResult> {
        let mut context = ToolContext {
            store: &mut self.store,
            engine: &mut self.engine,
            user_message: "",
            clipboard: &mut self.clipboard,
            allowed_tools: None,
        };
        SetPanTool.invoke(args, &mut context)
    }

    fn state(&self) -> session::SessionState {
        self.store.get(self.head()).expect("read head node").state
    }

    fn head(&self) -> session::NodeId {
        self.store.head().expect("session has a head")
    }

    fn node_count(&self) -> usize {
        self.store.list_nodes().expect("list history").len()
    }

    fn assert_one_node_appended_after(&self, prior_head: session::NodeId, prior_count: usize) {
        let new_head = self.head();
        assert_ne!(new_head, prior_head, "a valid edit should move the head");
        let node = self.store.get(new_head).expect("read appended node");
        assert_eq!(
            node.parent,
            Some(prior_head),
            "edit should append to prior head"
        );
        assert_eq!(
            self.node_count(),
            prior_count + 1,
            "edit should append one node"
        );
    }

    fn assert_history_unchanged(
        &self,
        prior_head: session::NodeId,
        prior_count: usize,
        prior_state: &session::SessionState,
    ) {
        assert_eq!(self.head(), prior_head, "refused call moved the head");
        assert_eq!(
            self.node_count(),
            prior_count,
            "refused call appended a node"
        );
        assert_eq!(
            &self.state(),
            prior_state,
            "refused call changed session state"
        );
    }
}

fn expect_ok(result: tools::Result<ToolResult>) -> Value {
    match result.expect("tool invocation") {
        ToolResult::Ok(value) => value,
        ToolResult::Error(message) => panic!("expected Ok, got Error({message})"),
    }
}

#[test]
fn dispatcher_sets_only_the_selected_tracks_pan_and_appends_one_node() {
    for track in 0..TRACK_COUNT {
        let mut session = Session::with_two_tracks(track);
        let prior_head = session.head();
        let prior_count = session.node_count();
        let mut expected = session.state();
        expected.tracks[track].pan = -0.5;

        let result = expect_ok(session.call("set_pan", json!({ "track": track, "pan": -0.5 })));

        assert_eq!(result["pan"], json!(-0.5));
        assert_eq!(result["node_id"], json!(session.head().to_hex()));
        assert_eq!(session.state(), expected, "selected track {track}");
        session.assert_one_node_appended_after(prior_head, prior_count);
    }
}

#[test]
fn dispatcher_rejects_pan_outside_both_bounds_without_mutation() {
    let mut session = Session::with_two_tracks(0);
    let prior_head = session.head();
    let prior_count = session.node_count();
    let prior_state = session.state();

    for pan in [-1.5, 1.5] {
        let error = session
            .call("set_pan", json!({ "track": 0, "pan": pan }))
            .expect_err("dispatcher should reject out-of-range pan");
        match error {
            DispatchError::SchemaValidation(message) => {
                assert!(message.contains("pan"), "pan {pan}: {message}");
            }
            other => panic!("expected pan schema validation, got {other:?}"),
        }
        session.assert_history_unchanged(prior_head, prior_count, &prior_state);
    }
}

#[test]
fn direct_tool_clamps_pan_to_each_boundary_and_preserves_other_state() {
    for track in 0..TRACK_COUNT {
        for (pan, clamped) in [(1.5, 1.0), (-2.0, -1.0)] {
            // Each edit needs a fresh history: revisiting an earlier state
            // can reuse its content-addressed node instead of adding one.
            let mut session = Session::with_two_tracks(track);
            let prior_head = session.head();
            let prior_count = session.node_count();
            let mut expected = session.state();
            expected.tracks[track].pan = clamped;

            let result =
                expect_ok(session.call_tool_directly(json!({ "track": track, "pan": pan })));

            assert_eq!(result["pan"], json!(clamped));
            assert_eq!(result["node_id"], json!(session.head().to_hex()));
            assert_eq!(session.state(), expected, "track {track}, pan {pan}");
            session.assert_one_node_appended_after(prior_head, prior_count);
        }
    }
}

#[test]
fn invalid_track_returns_tool_error_without_mutation() {
    let mut session = Session::with_two_tracks(0);
    let prior_head = session.head();
    let prior_count = session.node_count();
    let prior_state = session.state();

    let result = session
        .call("set_pan", json!({ "track": TRACK_COUNT, "pan": -0.5 }))
        .expect("valid arguments should pass dispatcher schema validation");

    match result {
        ToolResult::Error(message) => assert!(message.contains("track"), "{message}"),
        ToolResult::Ok(value) => panic!("expected invalid-track error, got Ok({value})"),
    }
    session.assert_history_unchanged(prior_head, prior_count, &prior_state);
}
