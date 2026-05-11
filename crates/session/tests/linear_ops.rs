//! Property tests for the linear `Store::append` subset, plus a crash-safety
//! integration test that spawns and SIGKILLs the `crash_writer` binary.

use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use chrono::Utc;
use proptest::prelude::*;
use session::{BusGraph, NodeId, SessionNode, SessionState, Store, TempoMap};
use tempfile::TempDir;

fn make_node(length_samples: u64) -> SessionNode {
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
        },
    }
}

#[test]
fn append_then_get_roundtrips() {
    let dir = TempDir::new().unwrap();
    let mut store = Store::open(dir.path()).unwrap();
    let id = store.append(make_node(1_000)).unwrap();
    let fetched = store.get(id).unwrap();
    assert_eq!(fetched.id, id);
    assert_eq!(fetched.state.length_samples, 1_000);
    assert_eq!(store.head(), Some(id));
}

#[test]
fn reopen_recovers_head() {
    let dir = TempDir::new().unwrap();
    let id = {
        let mut store = Store::open(dir.path()).unwrap();
        store.append(make_node(42)).unwrap()
    };
    let store = Store::open(dir.path()).unwrap();
    assert_eq!(store.head(), Some(id));
    let fetched = store.get(id).unwrap();
    assert_eq!(fetched.state.length_samples, 42);
}

#[test]
fn parent_chain_is_linear() {
    let dir = TempDir::new().unwrap();
    let mut store = Store::open(dir.path()).unwrap();
    let a = store.append(make_node(1)).unwrap();
    let b = store.append(make_node(2)).unwrap();
    let c = store.append(make_node(3)).unwrap();
    assert_eq!(store.get(a).unwrap().parent, None);
    assert_eq!(store.get(b).unwrap().parent, Some(a));
    assert_eq!(store.get(c).unwrap().parent, Some(b));
    assert_eq!(store.head(), Some(c));
}

#[test]
fn set_head_to_known_node() {
    let dir = TempDir::new().unwrap();
    let mut store = Store::open(dir.path()).unwrap();
    let a = store.append(make_node(1)).unwrap();
    let b = store.append(make_node(2)).unwrap();
    assert_eq!(store.head(), Some(b));
    store.set_head(a).unwrap();
    assert_eq!(store.head(), Some(a));
}

#[test]
fn empty_store_has_no_head() {
    let dir = TempDir::new().unwrap();
    let store = Store::open(dir.path()).unwrap();
    assert_eq!(store.head(), None);
}

#[test]
fn list_nodes_empty_store_returns_empty_vec() {
    let dir = TempDir::new().unwrap();
    let store = Store::open(dir.path()).unwrap();
    let nodes = store.list_nodes().unwrap();
    assert!(nodes.is_empty());
}

#[test]
fn list_nodes_returns_every_appended_node() {
    let dir = TempDir::new().unwrap();
    let mut store = Store::open(dir.path()).unwrap();
    let a = store.append(make_node(1)).unwrap();
    let b = store.append(make_node(2)).unwrap();
    let c = store.append(make_node(3)).unwrap();

    let mut ids: Vec<_> = store
        .list_nodes()
        .unwrap()
        .into_iter()
        .map(|n| n.id)
        .collect();
    ids.sort_by_key(|id| id.to_hex());
    let mut expected = vec![a, b, c];
    expected.sort_by_key(|id| id.to_hex());
    assert_eq!(ids, expected);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn append_sequence_is_readable(lengths in prop::collection::vec(1u64..1_000_000, 1..32)) {
        let dir = TempDir::new().unwrap();
        let mut store = Store::open(dir.path()).unwrap();
        let mut ids = Vec::with_capacity(lengths.len());
        for len in &lengths {
            let id = store.append(make_node(*len)).unwrap();
            ids.push(id);
            // After every append, head must be the most recent id.
            prop_assert_eq!(store.head(), Some(id));
        }
        // Every appended node is readable.
        for id in &ids {
            let _ = store.get(*id).unwrap();
        }
        // Reopening the store recovers the same head.
        drop(store);
        let store = Store::open(dir.path()).unwrap();
        prop_assert_eq!(store.head(), Some(*ids.last().unwrap()));
    }
}

/// Crash-safety: spawn the `crash_writer` binary (which spins on `append`
/// with NO inter-iteration sleep), kill it at many short delays so kills
/// land *inside* the rename window, and verify the on-disk store stays
/// consistent. The invariant: `head` always points to a node file that
/// exists on disk. Orphaned node files (written but head not yet updated)
/// are allowed — that is the recoverable side of the crash window.
///
/// To matter, the kill must land between the node-rename and the
/// head-rename, or partway through either tempfile write. We sweep many
/// short delays (0–10 ms) and run multiple trials per delay so a
/// regression that swapped the rename order would actually be caught.
#[test]
fn crash_during_append_leaves_consistent_store() {
    let bin = env!("CARGO_BIN_EXE_crash_writer");

    // 50 evenly spaced delays from ~0 µs to ~10 ms. The dangerous window
    // is roughly the first few hundred microseconds of an append (write
    // + fsync + rename + write + fsync + rename), so most of these will
    // hit somewhere inside `append`.
    const DELAY_COUNT: u64 = 50;
    const MAX_DELAY_US: u64 = 10_000;
    const TRIALS_PER_DELAY: u64 = 3;

    for i in 0..DELAY_COUNT {
        // Spread delays linearly: 0, ~204, ~408, ... µs.
        let delay_us = (i * MAX_DELAY_US) / DELAY_COUNT;
        for _ in 0..TRIALS_PER_DELAY {
            let dir = TempDir::new().unwrap();
            let mut child = Command::new(bin)
                .arg(dir.path())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .expect("spawn crash_writer");

            if delay_us > 0 {
                std::thread::sleep(Duration::from_micros(delay_us));
            }
            // SIGKILL on Unix; the child cannot trap it, so this
            // exercises the abrupt-death path.
            let _ = child.kill();
            let _ = child.wait();

            assert_store_consistent(dir.path());
        }
    }
}

fn assert_store_consistent(project_dir: &Path) {
    let store = Store::open(project_dir).expect("reopen store after crash");

    // No commits yet means the writer was killed before its first append
    // completed end-to-end. That's a valid state — head simply isn't set.
    let head_path = project_dir.join(".audiograph").join("head");
    if !head_path.exists() {
        return;
    }
    let raw = std::fs::read_to_string(&head_path).unwrap();
    if raw.trim().is_empty() {
        return;
    }

    // Head exists -> the corresponding node file MUST exist. This is the
    // critical crash-safety invariant.
    let head_id = store
        .head()
        .expect("head file present and non-empty but Store::head() returned None");
    let fetched = store
        .get(head_id)
        .expect("head points to a missing node file; crash safety property violated");
    assert_eq!(fetched.id, head_id);
}
