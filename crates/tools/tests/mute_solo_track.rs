//! Integration coverage for the mute and solo tools through the dispatcher.

use serde_json::json;
use tools::ToolResult;

#[path = "support/harness.rs"]
mod harness;
use harness::{expect_ok, Session};

const SAMPLE_RATE: u32 = 48_000;
const TRACK_COUNT: usize = 2;
const FRAMES: usize = 480;

impl Session {
    fn with_two_tracks() -> Self {
        let session = Session::with_tones(SAMPLE_RATE, FRAMES, &[(220.0, 0.488), (330.0, 0.488)]);
        assert_eq!(session.state().tracks.len(), TRACK_COUNT);
        session
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
                    session.try_call("solo_track", json!({ "track": soloed_track, "solo": true })),
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
                expect_ok(session.try_call(
                    "mute_track",
                    json!({ "track": seeded_track, "muted": !muted }),
                ));
            }
            let prior_head = session.head();
            let prior_count = session.node_count();
            let mut expected = session.state();
            expected.tracks[track].muted = muted;

            expect_ok(session.try_call("mute_track", json!({ "track": track, "muted": muted })));

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
            expect_ok(session.try_call("mute_track", json!({ "track": 0, "muted": true })));
            expect_ok(session.try_call("mute_track", json!({ "track": 1, "muted": false })));

            // Put both tracks in the opposite state so false must preserve
            // the other track's true flag, just as true preserves its false.
            // Seed the selected track first: the store content-addresses
            // states, so this keeps the measured result from reusing a state
            // already appended while preparing the fixture.
            for seeded_track in std::iter::once(track)
                .chain((0..TRACK_COUNT).filter(|candidate| *candidate != track))
            {
                expect_ok(session.try_call(
                    "solo_track",
                    json!({ "track": seeded_track, "solo": !solo }),
                ));
            }
            let prior_head = session.head();
            let prior_count = session.node_count();
            let mut expected = session.state();
            expected.tracks[track].soloed = solo;

            expect_ok(session.try_call("solo_track", json!({ "track": track, "solo": solo })));

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
        let result = session.try_call(tool, args).expect("dispatcher invocation");
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
                session.try_call(tool, args).is_err(),
                "dispatcher should reject missing required arguments for {tool}"
            );
            session.assert_history_unchanged(prior_head, prior_count, &prior_state);
        }
    }
}
