use serde::Deserialize;
use serde_json::{json, Value};
use session::audio_store::{AudioFolder, AudioStore};
use session::{Clip, Track, TrackId};

use crate::schema::anthropic_tool;
use crate::tool::util::{
    append_state, attach_render_warnings, load_head_state, parse_args, render_warnings_for_tracks,
    set_session_length,
};
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
            "Offline-render the selected tracks together and add the result as a new mixed track. track_indices selects which tracks to include. The result is a baked copy: it does not follow later edits to the tracks it came from. To put an effect such as a limiter on the whole mix, use add_effect with scope \"master\" instead of mixing down. Appends a new session node.",
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

    // Renders to a uniquely named temporary file and renames it to its content
    // hash; uses the offline renderer, not the engine (#421). A run that loses
    // the race to a user edit which changes the mix leaves its render in
    // `generated/`, which nothing sweeps.
    fn runs_off_the_lock(&self) -> bool {
        true
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match parse_args(args) {
            Ok(a) => a,
            Err(r) => return Ok(r),
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

        // Rendered straight into the audio store (#719) and named after the
        // render by the WAV's bytes, header included (the header holds the
        // rate and channel count): written whole, renamed into place, never
        // left under a name half-written.
        let mut report = None;
        let stored = AudioStore::new(ctx.store.project_dir()).put_named_by_content(
            AudioFolder::Generated,
            |tmp| {
                let r = audio_engine::render_state_to_wav(&filtered_state, tmp, None)
                    .map_err(|e| format!("render failed: {e}"))?;
                if r.frames_written == 0 {
                    return Err("render produced no audio".to_string());
                }
                report = Some(r);
                Ok(())
            },
        );
        let (file_name, cas_path) = match stored {
            Ok(stored) => stored,
            // A render's own failure says so; anything after it was storing.
            Err(e) if e.starts_with("render") => return Ok(ToolResult::Error(e)),
            Err(e) => return Ok(ToolResult::Error(format!("failed to store wav: {e}"))),
        };
        let Some(report) = report else {
            return Ok(ToolResult::Error("render produced no audio".to_string()));
        };
        let hash_bytes = file_name.digest().expect("a file's name is one hash");

        // What the mix left out. Asked of the original state rather than
        // read from the report, because the render was of a smaller state
        // whose tracks are numbered by their place in the selection, and the
        // model knows them by their place in the session. The audio is baked
        // into the new track, so an effect left out here is left out of it
        // for good, which is why this has to be said.
        let warnings = render_warnings_for_tracks(&state, &args.track_indices);

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
                volume_envelope_frames: session::EnvelopeFrames::default(),
            }],
            gain_db: 0.0,
            pan: 0.0,
            muted: false,
            soloed: false,
            effects: vec![],
            sends: Vec::new(),
        });
        set_session_length(&mut new_state);

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

        let mut out = json!({
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
        });
        attach_render_warnings(&mut out, &warnings);
        Ok(ToolResult::Ok(out))
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
