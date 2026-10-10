//! The path the user took through the history is kept with the project (#453).
//!
//! The frontend's reducer (`apps/desktop/src/lib/headTrail.ts`) decides
//! where undo and redo go; this file holds what it saves between runs.
//! These tests are about the file: that what was saved is what is read
//! back, that a file nobody can read is an empty trail and never an
//! error, and that what is read names only nodes the project still has.

use std::fs;

use chrono::Utc;
use session::trail::{read_trail, trail_path, write_trail, HeadTrail, MAX_TRAIL};
use session::{BusGraph, NodeId, SessionNode, SessionState, Store, TempoMap, STORE_DIR};
use tempfile::TempDir;

fn node(length_samples: u64) -> SessionNode {
    SessionNode {
        id: NodeId([0u8; 32]),
        parent: None,
        created_at: Utc::now(),
        label: None,
        reasoning: None,
        state: SessionState {
            tracks: Vec::new(),
            bus_routing: BusGraph::default(),
            master_chain: Vec::new(),
            tempo_map: TempoMap::default(),
            key_map: None,
            transcript: None,
            sample_rate: 48_000,
            length_samples,
            annotations: Vec::new(),
            sync_lock: false,
        },
        op: None,
    }
}

fn hex(id: NodeId) -> String {
    id.to_hex()
}

fn trail_of(head: NodeId, trail: &[NodeId], redo: &[NodeId]) -> HeadTrail {
    HeadTrail {
        head: Some(hex(head)),
        trail: trail.iter().copied().map(hex).collect(),
        redo: redo.iter().copied().map(hex).collect(),
    }
}

/// A project holding three distinct states, returned as `(dir, a, b, c)`.
fn project() -> (TempDir, NodeId, NodeId, NodeId) {
    let dir = TempDir::new().unwrap();
    let mut store = Store::open(dir.path()).unwrap();
    let a = store.append(node(1_000)).unwrap();
    let b = store.append(node(2_000)).unwrap();
    let c = store.append(node(3_000)).unwrap();
    (dir, a, b, c)
}

#[test]
fn a_saved_trail_reads_back_as_it_was_saved() {
    let (dir, a, b, c) = project();
    let saved = trail_of(c, &[a, b], &[b]);

    assert!(write_trail(dir.path(), &saved).unwrap());

    assert_eq!(read_trail(dir.path()), saved);
}

#[test]
fn the_file_is_beside_the_head_in_the_store_directory() {
    let (dir, _, _, c) = project();
    write_trail(dir.path(), &trail_of(c, &[], &[])).unwrap();

    assert_eq!(
        trail_path(dir.path()),
        dir.path().join(STORE_DIR).join("trail.json")
    );
    assert!(trail_path(dir.path()).is_file());
}

/// The reason the path is kept at all: mute, then unmute lands on a node
/// that already exists, and that node's stored parent is the step before
/// its FIRST visit. After a restart, only the saved path knows the mute.
#[test]
fn after_a_restart_the_path_still_holds_the_step_the_parent_does_not() {
    let dir = TempDir::new().unwrap();
    let (loaded, started, muted) = {
        let mut store = Store::open(dir.path()).unwrap();
        let loaded = store.append(node(1_000)).unwrap();
        let started = store.append(node(2_000)).unwrap();
        let muted = store.append(node(3_000)).unwrap();
        // Unmute: the state the project was in before the mute.
        assert_eq!(store.append(node(2_000)).unwrap(), started);
        (loaded, started, muted)
    };
    // The path the frontend holds after those edits: it was on `started`,
    // went to `muted`, and came back to `started`.
    write_trail(dir.path(), &trail_of(started, &[started, muted], &[])).unwrap();

    // The app is closed and opened again.
    let store = Store::open(dir.path()).unwrap();
    let restored = read_trail(dir.path());

    assert_eq!(store.head(), Some(started));
    assert_eq!(
        restored.head,
        Some(hex(started)),
        "the trail ends where the store's head is"
    );
    assert_eq!(
        restored.trail.last(),
        Some(&hex(muted)),
        "undo from here goes to the mute"
    );
    assert_eq!(
        store.get(started).unwrap().parent,
        Some(loaded),
        "the stored parent skips the mute, which is what #398 reported"
    );
}

#[test]
fn a_project_with_no_trail_file_has_an_empty_trail() {
    let (dir, _, _, _) = project();
    assert!(!trail_path(dir.path()).exists());
    assert_eq!(read_trail(dir.path()), HeadTrail::default());
}

#[test]
fn a_folder_with_no_store_at_all_has_an_empty_trail() {
    let dir = TempDir::new().unwrap();
    assert_eq!(read_trail(dir.path()), HeadTrail::default());
}

#[test]
fn a_file_that_cannot_be_read_is_an_empty_trail_and_not_an_error() {
    let (dir, a, _, c) = project();
    let path = trail_path(dir.path());
    let broken: [&[u8]; 8] = [
        b"",
        b"not json at all",
        b"{ \"head\": \"", // cut off mid-write
        b"[]",             // right syntax, wrong shape
        b"null",
        b"{\"head\": 7, \"trail\": \"x\"}",     // wrong types
        b"\xff\xfe\x00garbage",                 // not even UTF-8
        b"{\"head\": null, \"trail\": [1, 2]}", // entries that are not strings
    ];
    for bytes in broken {
        fs::write(&path, bytes).unwrap();
        assert_eq!(
            read_trail(dir.path()),
            HeadTrail::default(),
            "{:?}",
            String::from_utf8_lossy(bytes)
        );
    }

    // And the next save replaces it whole: a corrupt file does not make
    // the trail unwritable.
    fs::write(&path, b"not json at all").unwrap();
    let saved = trail_of(c, &[a], &[]);
    assert!(write_trail(dir.path(), &saved).unwrap());
    assert_eq!(read_trail(dir.path()), saved);
}

#[test]
fn a_file_that_is_a_directory_is_an_empty_trail() {
    let (dir, _, _, _) = project();
    fs::create_dir(trail_path(dir.path())).unwrap();
    assert_eq!(read_trail(dir.path()), HeadTrail::default());
}

#[test]
fn a_trail_whose_head_is_gone_is_empty() {
    let (dir, a, b, c) = project();
    write_trail(dir.path(), &trail_of(c, &[a, b], &[])).unwrap();
    Store::open(dir.path()).unwrap().remove_node(c).unwrap();

    assert_eq!(
        read_trail(dir.path()),
        HeadTrail::default(),
        "a path that does not end at a node of this project is not this project's path"
    );
}

#[test]
fn entries_for_nodes_that_are_gone_are_dropped_in_order() {
    // A compaction removes nodes. Undo must not be pointed at one: it
    // would be refused, and every press after it.
    let (dir, a, b, c) = project();
    write_trail(dir.path(), &trail_of(c, &[a, b, a, b], &[b, a])).unwrap();
    Store::open(dir.path()).unwrap().remove_node(b).unwrap();

    assert_eq!(read_trail(dir.path()), trail_of(c, &[a, a], &[a]));
}

#[test]
fn text_that_is_not_a_node_id_is_dropped() {
    let (dir, a, _, c) = project();
    // Two that are 64 bytes long and still not ids: a multi-byte character
    // that straddles a byte pair (slicing the text by pair would panic),
    // and signs in front of digits (which `from_str_radix` accepts).
    let wide = format!("0é{}", "0".repeat(61));
    assert_eq!(wide.len(), 64);
    let signed = "+f".repeat(32);
    let json = format!(
        r#"{{"head": "{c}", "trail": ["{a}", "zz", "", "../../etc/passwd", "{short}", "{wide}", "{signed}", "{a}"], "redo": ["nope"]}}"#,
        c = hex(c),
        a = hex(a),
        short = &hex(a)[..63],
    );
    fs::write(trail_path(dir.path()), json).unwrap();

    assert_eq!(read_trail(dir.path()), trail_of(c, &[a, a], &[]));
}

#[test]
fn text_that_is_not_a_node_id_is_not_written() {
    // The webview sends the trail, and the file is read back as ids.
    let (dir, a, _, c) = project();
    let trail = HeadTrail {
        head: Some(hex(c)),
        trail: vec![
            hex(a),
            "+f".repeat(32),
            "0é".to_string() + &"0".repeat(61),
            "x".into(),
        ],
        redo: vec!["../../etc/passwd".into()],
    };
    assert!(write_trail(dir.path(), &trail).unwrap());

    let on_disk: HeadTrail =
        serde_json::from_slice(&fs::read(trail_path(dir.path())).unwrap()).unwrap();
    assert_eq!(on_disk, trail_of(c, &[a], &[]));
}

#[test]
fn ids_are_read_back_in_the_form_the_backend_writes() {
    let (dir, a, _, c) = project();
    let json = format!(
        r#"{{"head": "{}", "trail": ["{}"]}}"#,
        hex(c).to_uppercase(),
        hex(a).to_uppercase()
    );
    fs::write(trail_path(dir.path()), json).unwrap();

    assert_eq!(read_trail(dir.path()), trail_of(c, &[a], &[]));
}

#[test]
fn the_fields_a_build_may_leave_out_read_as_empty() {
    // A file written by hand, or by a build that drops a field, must
    // still read.
    let (dir, a, _, c) = project();
    fs::write(
        trail_path(dir.path()),
        format!(r#"{{"head": "{}", "trail": ["{}"]}}"#, hex(c), hex(a)),
    )
    .unwrap();

    assert_eq!(read_trail(dir.path()), trail_of(c, &[a], &[]));
}

#[test]
fn a_long_history_is_bounded_to_the_most_recent_steps() {
    let (dir, a, b, c) = project();
    let cycle = [a, b, c];
    let long: Vec<NodeId> = (0..MAX_TRAIL + 120).map(|i| cycle[i % 3]).collect();

    assert!(write_trail(dir.path(), &trail_of(c, &long, &long)).unwrap());

    // The file itself is bounded, not only what is read from it.
    let on_disk: HeadTrail =
        serde_json::from_slice(&fs::read(trail_path(dir.path())).unwrap()).unwrap();
    assert_eq!(on_disk.trail.len(), MAX_TRAIL);
    assert_eq!(on_disk.redo.len(), MAX_TRAIL);

    // And what falls off is the far end: the newest steps stay.
    let expected: Vec<String> = long[long.len() - MAX_TRAIL..]
        .iter()
        .copied()
        .map(hex)
        .collect();
    let back = read_trail(dir.path());
    assert_eq!(back.trail, expected);
    assert_eq!(back.redo, expected);
}

#[test]
fn a_file_that_grew_past_the_bound_is_cut_when_read() {
    let (dir, a, _, c) = project();
    let many = vec![hex(a); MAX_TRAIL * 3];
    let json = serde_json::json!({ "head": hex(c), "trail": many, "redo": many });
    fs::write(trail_path(dir.path()), json.to_string()).unwrap();

    let back = read_trail(dir.path());
    assert_eq!(back.trail.len(), MAX_TRAIL);
    assert_eq!(back.redo.len(), MAX_TRAIL);
}

#[test]
fn a_file_far_larger_than_any_trail_is_not_read() {
    let (dir, a, _, c) = project();
    // Valid JSON, and a trail that would read fine, padded past anything
    // this code writes.
    let padding = " ".repeat(2 * 1024 * 1024);
    let json = format!(
        r#"{{"head": "{}", "trail": ["{}"]{padding}}}"#,
        hex(c),
        hex(a)
    );
    fs::write(trail_path(dir.path()), json).unwrap();

    assert_eq!(read_trail(dir.path()), HeadTrail::default());
}

#[test]
fn a_trail_for_another_project_is_not_written() {
    // The frontend saves without waiting, so a save sent for the project
    // being left can reach the backend after the next one has opened. Its
    // head is a node the new project does not have.
    let (dir, a, _, c) = project();
    let (other, x, _, _) = project_with_other_states();
    let kept = trail_of(c, &[a], &[]);
    write_trail(dir.path(), &kept).unwrap();

    assert!(
        !write_trail(dir.path(), &trail_of(x, &[], &[])).unwrap(),
        "refused"
    );
    assert_eq!(read_trail(dir.path()), kept, "and the file is as it was");
    drop(other);
}

fn project_with_other_states() -> (TempDir, NodeId, NodeId, NodeId) {
    let dir = TempDir::new().unwrap();
    let mut store = Store::open(dir.path()).unwrap();
    let a = store.append(node(7_000)).unwrap();
    let b = store.append(node(8_000)).unwrap();
    let c = store.append(node(9_000)).unwrap();
    (dir, a, b, c)
}

#[test]
fn a_trail_with_no_head_is_not_written() {
    let (dir, a, _, _) = project();
    let none = HeadTrail {
        head: None,
        trail: vec![hex(a)],
        redo: vec![],
    };
    assert!(!write_trail(dir.path(), &none).unwrap());
    assert!(!trail_path(dir.path()).exists());
}

#[test]
fn a_save_leaves_no_temporary_file_behind() {
    let (dir, a, b, c) = project();
    for n in 0..5 {
        write_trail(dir.path(), &trail_of(c, &[a, b][..n % 3], &[])).unwrap();
    }

    let store_dir = dir.path().join(STORE_DIR);
    let mut names: Vec<String> = fs::read_dir(&store_dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n != "nodes")
        .collect();
    names.sort();
    assert_eq!(names, ["head", "trail.json"], "found {names:?}");
}

#[test]
fn a_save_replaces_the_file_whole() {
    let (dir, a, b, c) = project();
    write_trail(dir.path(), &trail_of(c, &[a, b, a, b, a, b], &[a, b])).unwrap();
    let short = trail_of(b, &[a], &[]);
    write_trail(dir.path(), &short).unwrap();

    // A shorter trail written over a longer one leaves none of the longer.
    let text = fs::read_to_string(trail_path(dir.path())).unwrap();
    assert!(
        serde_json::from_str::<HeadTrail>(&text).is_ok(),
        "whole JSON: {text}"
    );
    assert_eq!(read_trail(dir.path()), short);
}
