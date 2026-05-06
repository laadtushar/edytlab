//! Test helper: appends nodes in a loop until killed.
//!
//! Used by the crash-safety integration test. The parent process spawns
//! this binary with the project dir as `argv[1]`, sleeps a randomized
//! interval, and SIGKILLs it. After the kill, the parent re-opens the
//! store and asserts the on-disk state is consistent.

use std::path::PathBuf;
use std::time::Duration;

use chrono::Utc;
use session::{BusGraph, NodeId, SessionNode, SessionState, Store, TempoMap};

fn main() {
    let project_dir = PathBuf::from(
        std::env::args()
            .nth(1)
            .expect("usage: crash_writer <project_dir>"),
    );
    let mut store = Store::open(&project_dir).expect("open store");

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
        // Tight loop with a brief yield so the parent has many kill
        // points to choose from rather than all landing post-loop.
        std::thread::sleep(Duration::from_micros(100));
    }
}
