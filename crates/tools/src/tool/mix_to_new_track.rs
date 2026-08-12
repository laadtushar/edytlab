use serde::Deserialize;
use serde_json::{json, Value};
use session::{Clip, Track, TrackId};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::schema::anthropic_tool;
use crate::tool::util::{append_state, load_head_state};
use crate::{Tool, ToolContext, ToolResult};

#[derive(Debug, Deserialize)]
struct Args {
    track_indices: Vec<usize>,
    name: Option<String>,
}

pub struct MixToNewTrackTool;

impl Tool for MixToNewTrackTool {
    fn name(&self) -> &'static str {
        "mix_to_new_track"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "mix_to_new_track",
            "Offline-render the selected tracks together and add the result as a new mixed track. track_indices selects which tracks to include. Appends a new session node.",
            json!({
                "type": "object",
                "properties": {
                    "track_indices": {
                        "type": "array",
                        "items": { "type": "integer" },
                        "description": "Indices of tracks to mix"
                    },
                    "name": {
                        "type": "string",
                        "default": "mix",
                        "description": "Name for the new track"
                    }
                },
                "required": ["track_indices"]
            }),
        )
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };

        if args.track_indices.is_empty() {
            return Ok(ToolResult::Error(
                "track_indices must not be empty".to_string(),
            ));
        }

        let name = args.name.unwrap_or_else(|| "mix".to_string());

        let state = match load_head_state(ctx) {
            Ok(s) => s,
            Err(e) => return Ok(ToolResult::Error(e)),
        };

        // Validate all indices before doing any work.
        for &idx in &args.track_indices {
            if idx >= state.tracks.len() {
                return Ok(ToolResult::Error(format!(
                    "track {idx} is out of range; session has {} track{}",
                    state.tracks.len(),
                    if state.tracks.len() == 1 { "" } else { "s" },
                )));
            }
        }

        // Build a filtered SessionState containing only the selected tracks,
        // with muted/soloed cleared so render_state_to_wav treats them equally.
        let mut filtered_state = state.clone();
        filtered_state.tracks = args
            .track_indices
            .iter()
            .map(|&idx| {
                let mut t = state.tracks[idx].clone();
                t.muted = false;
                t.soloed = false;
                t
            })
            .collect();

        // Render the filtered state to a temp file first.
        let gen_dir = ctx.store.project_dir().join("generated");
        if let Err(e) = std::fs::create_dir_all(&gen_dir) {
            return Ok(ToolResult::Error(format!("mkdir failed: {e}")));
        }

        // Generate unique temp filename to prevent concurrent collisions.
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let tid = format!("{:?}", std::thread::current().id());
        let tid_hash = {
            let mut h = blake3::Hasher::new();
            h.update(tid.as_bytes());
            h.finalize().to_hex()
        };
        let tmp_name = format!("__mix_tmp_{ts}_{tid_hash:.8}.wav");
        let tmp_path = gen_dir.join(tmp_name);
        let report = match audio_engine::render_state_to_wav(&filtered_state, &tmp_path, None) {
            Ok(r) => r,
            Err(e) => {
                let _ = std::fs::remove_file(&tmp_path);
                return Ok(ToolResult::Error(format!("render failed: {e}")));
            }
        };

        if report.frames_written == 0 {
            let _ = std::fs::remove_file(&tmp_path);
            return Ok(ToolResult::Error("render produced no audio".to_string()));
        }

        // Hash the rendered WAV bytes → CAS-rename.
        let wav_bytes = match std::fs::read(&tmp_path) {
            Ok(b) => b,
            Err(e) => {
                let _ = std::fs::remove_file(&tmp_path);
                return Ok(ToolResult::Error(format!(
                    "failed to read rendered wav: {e}"
                )));
            }
        };
        let hash = blake3::hash(&wav_bytes);
        let hash_bytes: [u8; 32] = *hash.as_bytes();
        let cas_path = gen_dir.join(format!("{}.wav", hash.to_hex()));

        if cas_path.exists() {
            let _ = std::fs::remove_file(&tmp_path);
        } else if let Err(e) = std::fs::rename(&tmp_path, &cas_path) {
            let _ = std::fs::remove_file(&tmp_path);
            return Ok(ToolResult::Error(format!("failed to store wav: {e}")));
        }

        // Add a new track pointing at the rendered WAV to the ORIGINAL state.
        let mut new_state = state;
        let track_idx = new_state.tracks.len();
        let n_frames = report.frames_written;
        new_state.tracks.push(Track {
            id: TrackId::new(),
            name: name.clone(),
            clips: vec![Clip {
                source_path: cas_path,
                start_in_track: 0,
                source_offset: 0,
                length: n_frames,
                content_hash: Some(hash_bytes),
                time_stretch_factor: None,
                pitch_shift_semitones: None,
                beat_grid: None,
                volume_envelope: vec![],
            }],
            gain_db: 0.0,
            pan: 0.0,
            muted: false,
            soloed: false,
            effects: vec![],
            sends: Vec::new(),
        });
        new_state.length_samples = new_state.length_samples.max(n_frames);

        let indices_str: Vec<String> = args.track_indices.iter().map(|i| i.to_string()).collect();
        let label = format!(
            "mix_to_new_track [{}] → \"{}\"",
            indices_str.join(", "),
            name
        );
        let new_id = match append_state(ctx, new_state, label) {
            Ok(id) => id,
            Err(e) => return Ok(ToolResult::Error(e)),
        };

        Ok(ToolResult::Ok(json!({
            "node_id": new_id.to_hex(),
            "track_index": track_idx,
            "frames_written": n_frames,
            "peak_dbfs": report.peak_dbfs,
            "summary": format!(
                "Mixed {} track{} into new track \"{}\" ({} frames)",
                args.track_indices.len(),
                if args.track_indices.len() == 1 { "" } else { "s" },
                name,
                n_frames,
            )
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::MixToNewTrackTool;
    use crate::Tool;

    #[test]
    fn tool_name() {
        assert_eq!(MixToNewTrackTool.name(), "mix_to_new_track");
    }
}
