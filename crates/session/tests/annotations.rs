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
        sync_lock: false,
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
        op: None,
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
        op: None,
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

// ---------------------------------------------------------------------------
// Task A4: add / remove helpers
// ---------------------------------------------------------------------------

#[test]
fn add_annotation_appends_node_with_extended_list() {
    let dir = TempDir::new().unwrap();
    let mut store = Store::open(dir.path()).unwrap();
    let head1 = store.append(empty_node()).unwrap();
    let m = marker("verse", 4.0);

    let head2 = store.add_annotation(head1, m.clone()).unwrap();
    assert_ne!(head1, head2, "adding an annotation must produce a new node");

    let visible_head2 = store.annotations_for(head2).unwrap();
    assert_eq!(visible_head2.len(), 1);
    assert_eq!(visible_head2[0], m);

    // The immutable graph promise: head1 is unaffected.
    let visible_head1 = store.annotations_for(head1).unwrap();
    assert!(visible_head1.is_empty());
}

#[test]
fn remove_annotation_appends_node_without_target() {
    let dir = TempDir::new().unwrap();
    let mut store = Store::open(dir.path()).unwrap();
    let head1 = store.append(empty_node()).unwrap();
    let m = marker("chorus", 8.0);

    let head2 = store.add_annotation(head1, m.clone()).unwrap();
    let head3 = store.remove_annotation(head2, m.id).unwrap();

    assert_ne!(head2, head3, "removal must produce a new node");

    let visible_head3 = store.annotations_for(head3).unwrap();
    assert!(visible_head3.is_empty());

    // History is immutable: head2 still has the marker.
    let visible_head2 = store.annotations_for(head2).unwrap();
    assert_eq!(visible_head2.len(), 1);
    assert_eq!(visible_head2[0], m);
}

#[test]
fn remove_annotation_unknown_id_is_idempotent() {
    let dir = TempDir::new().unwrap();
    let mut store = Store::open(dir.path()).unwrap();
    let head = store.append(empty_node()).unwrap();

    let same = store.remove_annotation(head, AnnotationId::new()).unwrap();
    assert_eq!(same, head, "removing a non-existent id must be a no-op");
}
