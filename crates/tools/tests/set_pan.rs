//! Pan edits through the dispatcher and the tool's schema-bypassing boundary.

use serde_json::{json, Value};
use tools::tool::SetPanTool;
use tools::{DispatchError, Tool, ToolContext, ToolResult};

#[path = "support/harness.rs"]
mod harness;
use harness::{expect_ok, Session};

const SAMPLE_RATE: u32 = 48_000;
const TRACK_COUNT: usize = 2;
const FRAMES: usize = 480;

impl Session {
    fn with_two_tracks(selected_track: usize) -> Self {
        let mut session =
            Session::with_tones(SAMPLE_RATE, FRAMES, &[(220.0, 0.488), (330.0, 0.488)]);
        assert_eq!(session.state().tracks.len(), TRACK_COUNT);

        let other_track = 1 - selected_track;
        expect_ok(session.try_call("set_pan", json!({ "track": other_track, "pan": 0.75 })));
        assert_eq!(session.state().tracks[other_track].pan, 0.75);
        assert_eq!(session.state().tracks[selected_track].pan, 0.0);
        session
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
}

#[test]
fn dispatcher_sets_only_the_selected_tracks_pan_and_appends_one_node() {
    for track in 0..TRACK_COUNT {
        let mut session = Session::with_two_tracks(track);
        let prior_head = session.head();
        let prior_count = session.node_count();
        let mut expected = session.state();
        expected.tracks[track].pan = -0.5;

        let result = expect_ok(session.try_call("set_pan", json!({ "track": track, "pan": -0.5 })));

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
            .try_call("set_pan", json!({ "track": 0, "pan": pan }))
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
        .try_call("set_pan", json!({ "track": TRACK_COUNT, "pan": -0.5 }))
        .expect("valid arguments should pass dispatcher schema validation");

    match result {
        ToolResult::Error(message) => assert!(message.contains("track"), "{message}"),
        ToolResult::Ok(value) => panic!("expected invalid-track error, got Ok({value})"),
    }
    session.assert_history_unchanged(prior_head, prior_count, &prior_state);
}
