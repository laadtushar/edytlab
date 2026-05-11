//! Session content (the part that gets content-hashed into a [`NodeId`]).
//!
//! Phase 1 only ever populates a 0-1-track session with empty bus routing
//! and master chain. The full struct shape is fixed now to keep the JSON
//! layout forward-compatible.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::annotation::Annotation;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TrackId(pub Uuid);

impl TrackId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for TrackId {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionState {
    pub tracks: Vec<Track>,
    pub bus_routing: BusGraph,
    pub master_chain: Vec<EffectInstance>,
    pub tempo_map: TempoMap,
    pub key_map: Option<KeyMap>,
    pub transcript: Option<Transcript>,
    pub sample_rate: u32,
    pub length_samples: u64,
    /// Marker / region annotations attached to the rendered audio at
    /// this head. `#[serde(default, skip_serializing_if = …)]` keeps
    /// existing on-disk node JSON readable AND keeps the content
    /// hash stable for nodes that don't use annotations.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub annotations: Vec<Annotation>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Track {
    pub id: TrackId,
    pub name: String,
    pub clips: Vec<Clip>,
    pub gain_db: f32,
    pub pan: f32,
    pub muted: bool,
    pub soloed: bool,
    pub effects: Vec<EffectInstance>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Clip {
    pub source_path: PathBuf,
    pub start_in_track: u64,
    pub source_offset: u64,
    pub length: u64,
    // blake3 of the source file bytes; optional because Phase 1 may not yet
    // know it at construction time. Pinned at render to lock provenance.
    #[serde(with = "crate::node::hex_array_32_opt")]
    pub content_hash: Option<[u8; 32]>,

    /// M20: requested time-stretch factor for this clip. `None` means
    /// no stretch; `Some(f)` means the audio engine should resample at
    /// render time to produce `length / f` output samples (the engine
    /// gains this capability in M22+; Phase 2 stub records the request
    /// only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_stretch_factor: Option<f32>,

    /// M20: requested pitch shift in semitones for this clip. `None`
    /// means no shift. Same render-time-application story as
    /// `time_stretch_factor`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pitch_shift_semitones: Option<f32>,

    /// M20: target beat grid (per-clip) for `align_to_beat`. Each entry
    /// is a beat time in seconds relative to the clip start; the engine
    /// will resample each beat-bound chunk to land on the target grid
    /// at render time. Phase 2 stub records the grid only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub beat_grid: Option<Vec<f32>>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BusGraph {
    pub buses: Vec<Bus>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Bus {
    pub id: Uuid,
    pub name: String,
    pub effects: Vec<EffectInstance>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EffectInstance {
    pub kind: String,
    pub params: serde_json::Value,
    pub bypassed: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TempoMap {
    pub default_bpm: f64,
    pub segments: Vec<TempoSegment>,
}

impl Default for TempoMap {
    fn default() -> Self {
        Self {
            default_bpm: 120.0,
            segments: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TempoSegment {
    pub start_sample: u64,
    pub bpm: f64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct KeyMap {
    pub segments: Vec<KeySegment>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KeySegment {
    pub start_sample: u64,
    pub key: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Transcript {
    pub words: Vec<TranscriptWord>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TranscriptWord {
    pub text: String,
    pub start_s: f32,
    pub end_s: f32,
    pub confidence: f32,
}

#[cfg(test)]
mod state_tests {
    //! Serde behaviour for the `annotations` field on `SessionState`.
    //!
    //! These tests pin the back-compat contract: pre-A2 node JSON without
    //! an `annotations` key must still deserialize, and empty-annotation
    //! states must serialise WITHOUT the field (so the content hash and
    //! existing snapshots stay stable for nodes that don't use them).
    use super::*;
    use crate::annotation::{Annotation, AnnotationId, AnnotationKind};

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

    #[test]
    fn annotations_default_to_empty() {
        // Legacy on-disk JSON written before A2 has no `annotations`
        // key. Must still deserialize.
        let legacy_json = serde_json::json!({
            "tracks": [],
            "bus_routing": { "buses": [] },
            "master_chain": [],
            "tempo_map": { "default_bpm": 120.0, "segments": [] },
            "key_map": null,
            "transcript": null,
            "sample_rate": 48000,
            "length_samples": 0,
        });
        let s: SessionState = serde_json::from_value(legacy_json).unwrap();
        assert!(s.annotations.is_empty());
    }

    #[test]
    fn empty_annotations_are_skipped_in_serialization() {
        // Empty Vec must not produce an `annotations` field — this is what
        // keeps `NodeId::from_state` stable for non-annotation nodes.
        let s = empty_state();
        let value = serde_json::to_value(&s).unwrap();
        let obj = value.as_object().expect("state serializes as object");
        assert!(
            !obj.contains_key("annotations"),
            "empty annotations leaked into JSON: {value}"
        );
    }

    #[test]
    fn non_empty_annotations_round_trip() {
        let mut s = empty_state();
        s.annotations.push(Annotation {
            id: AnnotationId::new(),
            name: "intro".into(),
            kind: AnnotationKind::Marker { time_sec: 1.5 },
        });
        let json = serde_json::to_string(&s).unwrap();
        let back: SessionState = serde_json::from_str(&json).unwrap();
        assert_eq!(back.annotations.len(), 1);
        assert_eq!(back, s);
    }
}
