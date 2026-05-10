//! `load` tool — decode a file and add a new track to the session.
//!
//! Phase 1 simplification was: `load` REPLACES the head's session state
//! with a fresh single-track session. M21 lifts that — a `load` against
//! an existing session APPENDS a new track. There is still no head?
//! Then we create a new session as before. The session-rate stays at the
//! first-loaded source's rate (subsequent loads at a different rate are
//! resampled to the project rate at render time, per M21 acceptance #3).

use std::path::PathBuf;

use chrono::Utc;
use serde::Deserialize;
use serde_json::{json, Value};
use session::{
    BusGraph, Clip, KeyMap, SessionNode, SessionState, TempoMap, Track, TrackId, Transcript,
};

use crate::schema::{anthropic_tool, object_schema};
use crate::{Tool, ToolContext, ToolResult};

#[derive(Debug, Deserialize)]
struct Args {
    path: String,
}

pub struct LoadTool;

impl Tool for LoadTool {
    fn name(&self) -> &'static str {
        "load"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "load",
            "Decode an audio file and add it to the session as a new track. With no current head this creates a fresh single-track session; otherwise the file is appended as a new track on the current head, leaving existing tracks intact. Returns the new session node id, the new track's index, and the source's sample rate, length, and channel count.",
            object_schema(&[("path", "string", true)]),
        )
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };

        let path = PathBuf::from(&args.path);
        if !path.exists() {
            return Ok(ToolResult::Error(format!(
                "file not found: {}",
                path.display()
            )));
        }

        let decoded = match audio_decoder::decode_file(&path) {
            Ok(d) => d,
            Err(e) => return Ok(ToolResult::Error(format!("decode failed: {e}"))),
        };

        let chans = decoded.channels as usize;
        if chans == 0 {
            return Ok(ToolResult::Error("decoded source has 0 channels".into()));
        }
        let length_samples = (decoded.samples.len() / chans) as u64;

        let track = Track {
            id: TrackId::new(),
            name: path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("track")
                .to_string(),
            clips: vec![Clip {
                source_path: path.clone(),
                start_in_track: 0,
                source_offset: 0,
                length: length_samples,
                content_hash: None,
                time_stretch_factor: None,
                pitch_shift_semitones: None,
                beat_grid: None,
            }],
            gain_db: 0.0,
            pan: 0.0,
            muted: false,
            soloed: false,
            effects: Vec::new(),
        };

        // If there's already a head, append; otherwise create a fresh session.
        let (state, track_index) = match ctx.store.head() {
            Some(head) => match ctx.store.get(head) {
                Ok(node) => {
                    let mut s = node.state;
                    let new_index = s.tracks.len();
                    s.tracks.push(track);
                    // The session's `length_samples` tracks the longest track
                    // so renders know the project's overall bound. Project
                    // sample rate is whatever the first load established —
                    // mismatched sources are resampled at render time.
                    if length_samples > s.length_samples {
                        s.length_samples = length_samples;
                    }
                    (s, new_index)
                }
                Err(e) => return Ok(ToolResult::Error(format!("failed to read head node: {e}"))),
            },
            None => {
                let s = SessionState {
                    tracks: vec![track],
                    bus_routing: BusGraph::default(),
                    master_chain: Vec::new(),
                    tempo_map: TempoMap::default(),
                    key_map: None::<KeyMap>,
                    transcript: None::<Transcript>,
                    sample_rate: decoded.sample_rate,
                    length_samples,
                    annotations: Vec::new(),
                };
                (s, 0)
            }
        };

        let node = SessionNode {
            // `parent` and `id` are overwritten by `Store::append`.
            id: session::NodeId([0u8; 32]),
            parent: None,
            created_at: Utc::now(),
            label: Some(format!("load {}", path.display())),
            reasoning: None,
            state,
        };

        let id = match ctx.store.append(node) {
            Ok(id) => id,
            Err(e) => return Ok(ToolResult::Error(format!("session append failed: {e}"))),
        };

        Ok(ToolResult::Ok(json!({
            "node_id": id.to_hex(),
            "track_index": track_index,
            "sample_rate": decoded.sample_rate,
            "length_samples": length_samples,
            "channels": decoded.channels,
            "summary": format!(
                "Loaded {} ({} ch, {} Hz, {} samples) as track {} on session head {}",
                path.display(),
                decoded.channels,
                decoded.sample_rate,
                length_samples,
                track_index,
                id.to_hex(),
            ),
        })))
    }
}
