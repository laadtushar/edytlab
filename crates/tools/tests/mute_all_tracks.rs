//! `mute_all_tracks` (#848, from #771 item 5): mute or unmute every track in
//! one call.

use serde_json::json;

#[path = "support/harness.rs"]
mod harness;
use harness::Session;

fn three() -> Session {
    Session::with_tones(8_000, 800, &[(100.0, 0.5), (200.0, 0.5), (300.0, 0.5)])
}

fn muted(s: &Session) -> Vec<bool> {
    s.state().tracks.iter().map(|t| t.muted).collect()
}

fn soloed(s: &Session) -> Vec<bool> {
    s.state().tracks.iter().map(|t| t.soloed).collect()
}

#[test]
fn muting_all_mutes_every_track_in_one_node() {
    let mut s = three();
    // One track already muted: the call sets all, it does not toggle.
    s.ok("mute_track", json!({ "track": 1, "muted": true }));
    let prior_head = s.head();
    let prior_count = s.node_count();

    let answer = s.ok("mute_all_tracks", json!({ "muted": true }));

    assert_eq!(muted(&s), [true, true, true]);
    s.assert_one_node_appended_after(prior_head, prior_count);
    assert_eq!(answer["node_id"], s.head_hex());
}

#[test]
fn unmuting_all_unmutes_every_track_and_undo_returns_to_the_earlier_head() {
    let mut s = three();
    let loaded_head = s.head();
    s.ok("mute_all_tracks", json!({ "muted": true }));
    let muted_head = s.head();
    assert_ne!(muted_head, loaded_head);
    assert_eq!(s.store.get(muted_head).unwrap().parent, Some(loaded_head));

    s.ok("mute_all_tracks", json!({ "muted": false }));

    // Node ids hash state only, so unmuting reaches the state the session
    // started from: the store hands back that first node, not a copy.
    assert_eq!(muted(&s), [false, false, false]);
    assert_eq!(s.head(), loaded_head);

    s.store.set_head(muted_head).unwrap();
    assert_eq!(
        muted(&s),
        [true, true, true],
        "undo is back on the muted node"
    );
}

#[test]
fn a_second_identical_call_changes_nothing_and_adds_no_node() {
    let mut s = three();
    s.ok("mute_all_tracks", json!({ "muted": true }));
    let head = s.head();
    let count = s.node_count();
    let state = s.state();

    let answer = s.ok("mute_all_tracks", json!({ "muted": true }));

    assert_eq!(s.state(), state);
    assert_eq!(s.head(), head, "nothing to change, so no new head");
    assert_eq!(s.node_count(), count);
    assert!(answer.get("node_id").is_none());
    assert!(
        answer["summary"].as_str().unwrap().contains("already"),
        "the answer says why: {answer}"
    );
}

#[test]
fn solo_flags_are_left_alone() {
    let mut s = three();
    s.ok("solo_track", json!({ "track": 0, "solo": true }));
    s.ok("solo_track", json!({ "track": 2, "solo": true }));
    let before = soloed(&s);
    assert_eq!(before, [true, false, true]);

    s.ok("mute_all_tracks", json!({ "muted": true }));
    assert_eq!(soloed(&s), before);
    assert_eq!(muted(&s), [true, true, true]);

    s.ok("mute_all_tracks", json!({ "muted": false }));
    assert_eq!(soloed(&s), before);
    assert_eq!(muted(&s), [false, false, false]);
}

#[test]
fn only_the_mute_flag_changes() {
    let mut s = three();
    let mut expected = s.state();
    for t in &mut expected.tracks {
        t.muted = true;
    }
    s.ok("mute_all_tracks", json!({ "muted": true }));
    assert_eq!(s.state(), expected);
}

#[test]
fn a_session_with_no_tracks_is_ok_and_adds_no_node() {
    let mut s = Session::with_tones(8_000, 800, &[(100.0, 0.5)]);
    s.ok("remove_track", json!({ "track": 0 }));
    assert!(s.state().tracks.is_empty());
    let head = s.head();
    let count = s.node_count();

    let answer = s.ok("mute_all_tracks", json!({ "muted": true }));

    assert_eq!(s.head(), head);
    assert_eq!(s.node_count(), count);
    assert!(answer.get("node_id").is_none());
    assert!(
        answer["summary"].as_str().unwrap().contains("no tracks"),
        "the answer says so: {answer}"
    );
}

#[test]
fn a_project_with_no_session_is_refused_like_the_other_track_tools() {
    let mut s = Session::empty();
    assert!(s
        .err("mute_all_tracks", json!({ "muted": true }))
        .contains("no session loaded"));
}

#[test]
fn muted_is_required() {
    let mut s = three();
    let head = s.head();
    assert!(s.try_call("mute_all_tracks", json!({})).is_err());
    assert!(s
        .try_call("mute_all_tracks", json!({ "muted": "yes" }))
        .is_err());
    assert_eq!(s.head(), head);
}
