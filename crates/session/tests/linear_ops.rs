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
    assert_eq!(store.head(), id);
}

#[test]
fn reopen_recovers_head() {
    let dir = TempDir::new().unwrap();
    let id = {
        let mut store = Store::open(dir.path()).unwrap();
        store.append(make_node(42)).unwrap()
    };
    let store = Store::open(dir.path()).unwrap();
    assert_eq!(store.head(), id);
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
    assert_eq!(store.head(), c);
}

#[test]
fn set_head_to_known_node() {
    let dir = TempDir::new().unwrap();
    let mut store = Store::open(dir.path()).unwrap();
    let a = store.append(make_node(1)).unwrap();
    let b = store.append(make_node(2)).unwrap();
    assert_eq!(store.head(), b);
    store.set_head(a).unwrap();
    assert_eq!(store.head(), a);
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
            prop_assert_eq!(store.head(), id);
        }
        // Every appended node is readable.
        for id in &ids {
            let _ = store.get(*id).unwrap();
        }
        // Reopening the store recovers the same head.
        drop(store);
        let store = Store::open(dir.path()).unwrap();
        prop_assert_eq!(store.head(), *ids.last().unwrap());
    }
}

/// Crash-safety: spawn the `crash_writer` binary, kill it after a randomized
/// delay, and verify the on-disk store is in a valid state. The invariant:
/// `head` always points to a node file that exists on disk. Orphaned node
/// files (written but head not yet updated) are allowed — that is the
/// recoverable side of the crash window.
#[test]
fn crash_during_append_leaves_consistent_store() {
    let bin = env!("CARGO_BIN_EXE_crash_writer");

    // Each iteration uses a different sleep so we sweep the kill window
    // across the append phases (tempfile write, rename, head rename).
    let delays_ms = [5u64, 15, 30, 60, 120, 200];

    for delay in delays_ms {
        let dir = TempDir::new().unwrap();
        let mut child = Command::new(bin)
            .arg(dir.path())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn crash_writer");

        std::thread::sleep(Duration::from_millis(delay));
        // SIGKILL — Tokio/std `kill()` on Unix sends SIGKILL, which the
        // child cannot trap, so this exercises the abrupt-death path.
        let _ = child.kill();
        let _ = child.wait();

        assert_store_consistent(dir.path());
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
    let head_id = store.head();
    let fetched = store
        .get(head_id)
        .expect("head points to a missing node file; crash safety property violated");
    assert_eq!(fetched.id, head_id);
}
