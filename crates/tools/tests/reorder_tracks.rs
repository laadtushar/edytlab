//! `reorder_tracks` (#771): move one track to a new index.

use serde_json::json;

#[path = "support/harness.rs"]
mod harness;
use harness::Session;

fn names(s: &Session) -> Vec<String> {
    s.state().tracks.iter().map(|t| t.name.clone()).collect()
}

fn three() -> Session {
    let mut s = Session::with_tones(8_000, 800, &[(100.0, 0.5), (200.0, 0.5), (300.0, 0.5)]);
    for (i, n) in ["a", "b", "c"].into_iter().enumerate() {
        s.ok("rename_track", json!({ "track": i, "name": n }));
    }
    s
}

#[test]
fn a_track_moves_to_the_new_index_and_the_others_shift() {
    let mut s = three();
    let ids: Vec<_> = s.state().tracks.iter().map(|t| t.id.clone()).collect();
    s.ok("reorder_tracks", json!({ "track": 2, "new_index": 0 }));
    assert_eq!(names(&s), ["c", "a", "b"]);
    let after: Vec<_> = s.state().tracks.iter().map(|t| t.id.clone()).collect();
    assert_eq!(
        after,
        [ids[2].clone(), ids[0].clone(), ids[1].clone()],
        "ids travel with their tracks"
    );

    s.ok("reorder_tracks", json!({ "track": 0, "new_index": 2 }));
    assert_eq!(names(&s), ["a", "b", "c"]);
}

#[test]
fn a_move_is_one_node_and_undo_restores_the_order() {
    let mut s = three();
    let before = s
        .state()
        .tracks
        .iter()
        .map(|t| t.id.clone())
        .collect::<Vec<_>>();
    let head = s.store.head().unwrap();
    s.ok("reorder_tracks", json!({ "track": 1, "new_index": 2 }));
    let node = s.store.get(s.store.head().unwrap()).unwrap();
    assert_eq!(node.parent, Some(head), "one node on the old head");
    s.store.set_head(head).unwrap();
    let back = s
        .state()
        .tracks
        .iter()
        .map(|t| t.id.clone())
        .collect::<Vec<_>>();
    assert_eq!(back, before);
}

#[test]
fn out_of_range_and_no_op_moves_are_refused_without_a_node() {
    let mut s = three();
    let head = s.store.head().unwrap();
    assert!(s
        .err("reorder_tracks", json!({ "track": 3, "new_index": 0 }))
        .contains("out of range"));
    assert!(s
        .err("reorder_tracks", json!({ "track": 0, "new_index": 3 }))
        .contains("out of range"));
    assert!(s
        .err("reorder_tracks", json!({ "track": 1, "new_index": 1 }))
        .contains("already at index 1"));
    assert_eq!(s.store.head().unwrap(), head, "a refusal adds no node");
}
