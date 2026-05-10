//! Integration tests for the marker / region annotation store helpers
//! introduced in the audacity-surface plan (Tasks A3 and A4).
//!
//! These exercise `Store::annotations_for` (and, after A4, the
//! `add_annotation` / `remove_annotation` helpers) against a real
//! on-disk store, both for the happy paths and the edge cases.

use chrono::Utc;
use session::{
    Annotation, AnnotationId, AnnotationKind, BusGraph, NodeId, SessionNode, SessionState, Store,
    TempoMap,
};
use tempfile::TempDir;

fn empty_state() -> SessionState {
    SessionState {
        tracks: Vec::new(),
        bus_routing: BusGraph::default(),
        master_chain: Vec::new(),
        tempo_map: TempoMap::default(),
        key_map: None,
        transcript: None,
        sample_rate: 48_000,
        length_samples: 0,
        annotations: Vec::new(),
    }
}

fn empty_node() -> SessionNode {
    SessionNode {
        id: NodeId([0u8; 32]),
        parent: None,
        created_at: Utc::now(),
        label: None,
        reasoning: None,
        state: empty_state(),
    }
}

fn node_with_annotations(annotations: Vec<Annotation>) -> SessionNode {
    let mut state = empty_state();
    state.annotations = annotations;
    SessionNode {
        id: NodeId([0u8; 32]),
        parent: None,
        created_at: Utc::now(),
        label: None,
        reasoning: None,
        state,
    }
}

fn marker(name: &str, time_sec: f64) -> Annotation {
    Annotation {
        id: AnnotationId::new(),
        name: name.into(),
        kind: AnnotationKind::Marker { time_sec },
    }
}

#[test]
fn annotations_for_returns_head_state_annotations() {
    let dir = TempDir::new().unwrap();
    let mut store = Store::open(dir.path()).unwrap();
    let m = marker("intro", 1.5);
    let id = store
        .append(node_with_annotations(vec![m.clone()]))
        .unwrap();

    let visible = store.annotations_for(id).unwrap();
    assert_eq!(visible.len(), 1);
    assert_eq!(visible[0], m);
}

#[test]
fn annotations_for_at_empty_head_is_empty() {
    let dir = TempDir::new().unwrap();
    let mut store = Store::open(dir.path()).unwrap();
    let id = store.append(empty_node()).unwrap();

    let visible = store.annotations_for(id).unwrap();
    assert!(visible.is_empty());
}
