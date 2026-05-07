//! Test helper: appends nodes in a loop until killed.
//!
//! Used by the crash-safety integration test. The parent process spawns
//! this binary with the project dir as `argv[1]`, sleeps a randomized
//! interval, and SIGKILLs it. After the kill, the parent re-opens the
//! store and asserts the on-disk state is consistent.

use std::path::PathBuf;

use chrono::Utc;
use session::{BusGraph, NodeId, SessionNode, SessionState, Store, TempoMap};

fn main() {
    let project_dir = PathBuf::from(
        std::env::args()
            .nth(1)
            .expect("usage: crash_writer <project_dir>"),
    );
    let mut store = Store::open(&project_dir).expect("open store");

    // Tight spin: NO inter-iteration sleep. The crash test's whole point
    // is to land SIGKILLs *inside* `append`'s critical section (between
    // the node-rename and the head-rename, or partway through either
    // tempfile write). Any sleep here lets most kills land in the gap
    // and turns the test into theater.
    for i in 0u64.. {
        let state = SessionState {
            tracks: Vec::new(),
            bus_routing: BusGraph::default(),
            master_chain: Vec::new(),
            tempo_map: TempoMap::default(),
            key_map: None,
            transcript: None,
            sample_rate: 48_000,
            // Vary length so each state has a distinct content hash.
            length_samples: i,
        };
        let node = SessionNode {
            id: NodeId([0u8; 32]),
            parent: None,
            created_at: Utc::now(),
            label: Some(format!("crash_writer iter {i}")),
            reasoning: None,
            state,
        };
        store.append(node).expect("append");
    }
}
