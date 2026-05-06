//! Session content (the part that gets content-hashed into a [`NodeId`]).
//!
//! Phase 1 only ever populates a 0-1-track session with empty bus routing
//! and master chain. The full struct shape is fixed now to keep the JSON
//! layout forward-compatible.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionState {
    pub tracks: Vec<Track>,
    pub bus_routing: BusGraph,
    pub master_chain: Vec<EffectInstance>,
    pub tempo_map: TempoMap,
    pub key_map: Option<KeyMap>,
    pub transcript: Option<Transcript>,
    pub sample_rate: u32,
    pub length_samples: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Clip {
    pub source_path: PathBuf,
    pub start_in_track: u64,
    pub source_offset: u64,
    pub length: u64,
    // blake3 of the source file bytes; optional because Phase 1 may not yet
    // know it at construction time. Pinned at render to lock provenance.
    #[serde(with = "crate::node::hex_array_32_opt")]
    pub content_hash: Option<[u8; 32]>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BusGraph {
    pub buses: Vec<Bus>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bus {
    pub id: Uuid,
    pub name: String,
    pub effects: Vec<EffectInstance>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EffectInstance {
    pub kind: String,
    pub params: serde_json::Value,
    pub bypassed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TempoSegment {
    pub start_sample: u64,
    pub bpm: f64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct KeyMap {
    pub segments: Vec<KeySegment>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeySegment {
    pub start_sample: u64,
    pub key: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Transcript {
    pub words: Vec<TranscriptWord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptWord {
    pub text: String,
    pub start_s: f32,
    pub end_s: f32,
    pub confidence: f32,
}
