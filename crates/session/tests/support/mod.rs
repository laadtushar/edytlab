//! Values the session store tests build over and over: an empty state, a
//! node around it, and a clip and a track to put in it.
//!
//! A test binary includes this with `mod support;` and uses a subset. A test
//! that opens its stores some other way (`Store::open_private`,
//! `Registry::with_identity`, the raw `Store::open`) keeps doing so itself;
//! nothing here opens a store.
#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};

use chrono::Utc;
use session::{
    Annotation, AnnotationId, AnnotationKind, BusGraph, Clip, NodeId, NodeOp, SessionNode,
    SessionState, TempoMap, Track, TrackId, STORE_DIR,
};

/// A session with no tracks, 48 kHz, `length_samples` long.
pub fn state_of_length(length_samples: u64) -> SessionState {
    SessionState {
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
    }
}

/// A session with no tracks and no length.
pub fn empty_state() -> SessionState {
    state_of_length(0)
}

/// A node holding `state`, with no parent, label or op. Its id is a
/// placeholder: the store names the node when it appends it.
pub fn node_with(state: SessionState) -> SessionNode {
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

/// A node of an empty session `length_samples` long.
pub fn node(length_samples: u64) -> SessionNode {
    node_with(state_of_length(length_samples))
}

/// A node of an empty session.
pub fn empty_node() -> SessionNode {
    node_with(empty_state())
}

pub fn op(tool: &str) -> NodeOp {
    NodeOp::new(tool.into(), serde_json::json!({}), "test".into())
}

/// A clip of `length` frames of the file at `path`, placed at the start.
pub fn clip_of(path: &Path, length: u64) -> Clip {
    Clip {
        source_path: path.to_path_buf(),
        start_in_track: 0,
        source_offset: 0,
        length,
        content_hash: None,
        time_stretch_factor: None,
        pitch_shift_semitones: None,
        beat_grid: None,
        volume_envelope: Vec::new(),
        volume_envelope_frames: Default::default(),
    }
}

/// A track holding `clips`: unity gain, centred, audible, no chain, no sends.
pub fn track_of(id: TrackId, name: &str, clips: Vec<Clip>) -> Track {
    Track {
        id,
        name: name.into(),
        clips,
        gain_db: 0.0,
        pan: 0.0,
        muted: false,
        soloed: false,
        effects: Vec::new(),
        sends: Vec::new(),
    }
}

/// The files under `<project>/.audiograph/`, as `(relative path, bytes)`,
/// sorted: what "nothing was written" is checked against.
pub fn snapshot(project: &Path) -> Vec<(String, Vec<u8>)> {
    fn walk(dir: &Path, root: &Path, out: &mut Vec<(String, Vec<u8>)>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(&path, root, out);
            } else {
                let rel = path.strip_prefix(root).unwrap().display().to_string();
                out.push((rel, fs::read(&path).unwrap()));
            }
        }
    }
    let root = project.join(STORE_DIR);
    let mut out = Vec::new();
    walk(&root, &root, &mut out);
    out.sort();
    out
}

/// Where the store keeps node `id` under `project`.
pub fn node_file(project: &Path, id: NodeId) -> PathBuf {
    let hex = id.to_hex();
    project
        .join(STORE_DIR)
        .join("nodes")
        .join(&hex[0..2])
        .join(format!("{hex}.json"))
}

/// A marker named `name` at `time_sec`.
pub fn marker_at(name: &str, time_sec: f64) -> Annotation {
    Annotation {
        id: AnnotationId::new(),
        name: name.into(),
        kind: AnnotationKind::Marker { time_sec },
    }
}
