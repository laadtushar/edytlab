//! Integration coverage for the mute and solo tools through the dispatcher.

use std::path::{Path, PathBuf};

use hound::{SampleFormat, WavSpec, WavWriter};
use serde_json::{json, Value};
use tempfile::TempDir;
use tools::{ToolContext, ToolDispatcher, ToolResult};

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
    fn with_two_tracks() -> Self {
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

    fn state(&self) -> session::SessionState {
        let head = self.store.head().expect("session has a head");
        self.store.get(head).expect("read head node").state
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

fn expect_ok(result: tools::Result<ToolResult>) {
    match result.expect("dispatcher invocation") {
        ToolResult::Ok(_) => {}
        ToolResult::Error(message) => panic!("expected Ok, got Error({message})"),
    }
}

#[test]
fn mute_track_changes_only_the_requested_tracks_mute_flag() {
    for track in 0..TRACK_COUNT {
        for muted in [true, false] {
            let mut session = Session::with_two_tracks();

            // Existing solo flags must survive a mute edit.
            for soloed_track in 0..TRACK_COUNT {
                expect_ok(
                    session.call("solo_track", json!({ "track": soloed_track, "solo": true })),
                );
            }

            // Put both tracks in the opposite state so false must preserve
            // the other track's true flag, just as true preserves its false.
            // Seed the selected track first: the store content-addresses
            // states, so this keeps the measured result from reusing a state
            // already appended while preparing the fixture.
            for seeded_track in std::iter::once(track)
                .chain((0..TRACK_COUNT).filter(|candidate| *candidate != track))
            {
                expect_ok(session.call(
                    "mute_track",
                    json!({ "track": seeded_track, "muted": !muted }),
                ));
            }
            let prior_head = session.head();
            let prior_count = session.node_count();
            let mut expected = session.state();
            expected.tracks[track].muted = muted;

            expect_ok(session.call("mute_track", json!({ "track": track, "muted": muted })));

            assert_eq!(session.state(), expected, "track {track}, muted={muted}");
            session.assert_one_node_appended_after(prior_head, prior_count);
        }
    }
}

#[test]
fn solo_track_changes_only_the_requested_tracks_solo_flag() {
    for track in 0..TRACK_COUNT {
        for solo in [true, false] {
            let mut session = Session::with_two_tracks();

            // Existing mute flags must survive a solo edit.
            expect_ok(session.call("mute_track", json!({ "track": 0, "muted": true })));
            expect_ok(session.call("mute_track", json!({ "track": 1, "muted": false })));

            // Put both tracks in the opposite state so false must preserve
            // the other track's true flag, just as true preserves its false.
            // Seed the selected track first: the store content-addresses
            // states, so this keeps the measured result from reusing a state
            // already appended while preparing the fixture.
            for seeded_track in std::iter::once(track)
                .chain((0..TRACK_COUNT).filter(|candidate| *candidate != track))
            {
                expect_ok(session.call(
                    "solo_track",
                    json!({ "track": seeded_track, "solo": !solo }),
                ));
            }
            let prior_head = session.head();
            let prior_count = session.node_count();
            let mut expected = session.state();
            expected.tracks[track].soloed = solo;

            expect_ok(session.call("solo_track", json!({ "track": track, "solo": solo })));

            assert_eq!(session.state(), expected, "track {track}, solo={solo}");
            session.assert_one_node_appended_after(prior_head, prior_count);
        }
    }
}

#[test]
fn out_of_range_track_is_rejected_without_changing_history() {
    for (tool, flag) in [("mute_track", "muted"), ("solo_track", "solo")] {
        let mut session = Session::with_two_tracks();
        let prior_head = session.head();
        let prior_count = session.node_count();
        let prior_state = session.state();

        let args = if flag == "muted" {
            json!({ "track": TRACK_COUNT, "muted": true })
        } else {
            json!({ "track": TRACK_COUNT, "solo": true })
        };
        let result = session.call(tool, args).expect("dispatcher invocation");
        assert!(
            matches!(result, ToolResult::Error(_)),
            "{tool} should return ToolResult::Error for an out-of-range track"
        );
        session.assert_history_unchanged(prior_head, prior_count, &prior_state);
    }
}

#[test]
fn missing_required_track_or_flag_is_refused_without_changing_history() {
    for (tool, missing_track, missing_flag) in [
        (
            "mute_track",
            json!({ "muted": true }),
            json!({ "track": 0 }),
        ),
        ("solo_track", json!({ "solo": true }), json!({ "track": 0 })),
    ] {
        let mut session = Session::with_two_tracks();
        for args in [missing_track, missing_flag] {
            let prior_head = session.head();
            let prior_count = session.node_count();
            let prior_state = session.state();

            assert!(
                session.call(tool, args).is_err(),
                "dispatcher should reject missing required arguments for {tool}"
            );
            session.assert_history_unchanged(prior_head, prior_count, &prior_state);
        }
    }
}
