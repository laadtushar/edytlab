//! `load` tool — decode a file and replace the session state with a
//! fresh single-track session whose only clip references the file.
//!
//! Phase 1 simplification: `load` REPLACES the head's session state. The
//! demo loads one file at a time and a multi-`load` workflow is out of
//! scope. The forward-compat path (multi-track via additive `load`) lives
//! behind the same tool name in Phase 2.

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
            "Decode an audio file and replace the current session with a single-track session referencing it. Returns the new session node id, sample rate, length in samples, and channel count.",
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

        let state = SessionState {
            tracks: vec![track],
            bus_routing: BusGraph::default(),
            master_chain: Vec::new(),
            tempo_map: TempoMap::default(),
            key_map: None::<KeyMap>,
            transcript: None::<Transcript>,
            sample_rate: decoded.sample_rate,
            length_samples,
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
            "sample_rate": decoded.sample_rate,
            "length_samples": length_samples,
            "channels": decoded.channels,
            "summary": format!(
                "Loaded {} ({} ch, {} Hz, {} samples) as new session head {}",
                path.display(),
                decoded.channels,
                decoded.sample_rate,
                length_samples,
                id.to_hex(),
            ),
        })))
    }
}
