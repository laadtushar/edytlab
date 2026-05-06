//! `normalize` — scale a track so its peak amplitude equals
//! `target_dbfs`.
//!
//! The track's audio is *not* re-rendered; instead we scan the source,
//! find the existing peak (in linear amplitude), then SET
//! `Track::gain_db` to the value that brings that peak to
//! `target_dbfs`. The audio engine applies the gain at render time, so
//! the on-disk source stays untouched and the operation is reversible
//! by appending a new node.
//!
//! Phase 1 simplification: we look at the FIRST clip's source file. The
//! plan only ever exercises a single-clip track, so this matches the
//! `RenderGraph::source_path` invariant.

use serde::Deserialize;
use serde_json::{json, Value};

use crate::schema::{anthropic_tool, object_schema};
use crate::tool::util::{append_state, check_track_index, load_head_state};
use crate::{Tool, ToolContext, ToolResult};

#[derive(Debug, Deserialize)]
struct Args {
    track: usize,
    target_dbfs: f32,
}

pub struct NormalizeTool;

impl Tool for NormalizeTool {
    fn name(&self) -> &'static str {
        "normalize"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "normalize",
            "Scan a track for its peak amplitude and set its gain so the peak equals target_dbfs. The source file is not rewritten; the engine applies the gain at render time. Appends a new session node parented to the current head.",
            object_schema(&[
                ("track", "integer", true),
                ("target_dbfs", "number", true),
            ]),
        )
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };

        if !args.target_dbfs.is_finite() {
            return Ok(ToolResult::Error(format!(
                "target_dbfs must be finite; got {}",
                args.target_dbfs
            )));
        }
        if args.target_dbfs > 0.0 {
            return Ok(ToolResult::Error(format!(
                "target_dbfs must be <= 0.0; got {}",
                args.target_dbfs
            )));
        }

        let mut state = match load_head_state(ctx) {
            Ok(s) => s,
            Err(msg) => return Ok(ToolResult::Error(msg)),
        };

        if let Err(msg) = check_track_index(&state.tracks, args.track) {
            return Ok(ToolResult::Error(msg));
        }

        let track = &mut state.tracks[args.track];
        let Some(clip) = track.clips.first().cloned() else {
            return Ok(ToolResult::Error(
                "track has no clips; nothing to normalize".into(),
            ));
        };

        let decoded = match audio_decoder::decode_file(&clip.source_path) {
            Ok(d) => d,
            Err(e) => return Ok(ToolResult::Error(format!("decode failed: {e}"))),
        };
        let chans = decoded.channels as usize;
        if chans == 0 {
            return Ok(ToolResult::Error("decoded source has 0 channels".into()));
        }

        // Scan only the clip's slice of the source — `source_offset` /
        // `length` are in frames, not interleaved samples.
        let frames_total = decoded.samples.len() / chans;
        let clip_start = (clip.source_offset as usize).min(frames_total);
        let clip_end =
            ((clip.source_offset.saturating_add(clip.length)) as usize).min(frames_total);
        let mut peak: f32 = 0.0;
        for frame in clip_start..clip_end {
            for ch in 0..chans {
                let s = decoded.samples[frame * chans + ch].abs();
                if s > peak {
                    peak = s;
                }
            }
        }

        if peak <= 0.0 {
            return Ok(ToolResult::Error(
                "track is silent; cannot normalize".into(),
            ));
        }

        // Required linear gain = 10^(target_dbfs / 20) / peak.
        // In dB: target_dbfs - 20*log10(peak).
        let current_peak_dbfs = 20.0 * peak.log10();
        let needed_db = args.target_dbfs - current_peak_dbfs;

        // Replace existing gain (don't compose) — normalize is an
        // absolute operation; the user wants the peak at exactly
        // target_dbfs after this call.
        track.gain_db = needed_db;

        let new_id = match append_state(
            ctx,
            state,
            format!(
                "normalize track {} -> {} dBFS",
                args.track, args.target_dbfs
            ),
        ) {
            Ok(id) => id,
            Err(msg) => return Ok(ToolResult::Error(msg)),
        };

        Ok(ToolResult::Ok(json!({
            "node_id": new_id.to_hex(),
            "applied_gain_db": needed_db,
            "source_peak_dbfs": current_peak_dbfs,
            "summary": format!(
                "Normalized track {} to {} dBFS (peak was {:.2} dBFS, applied {:+.2} dB); new head {}",
                args.track, args.target_dbfs, current_peak_dbfs, needed_db, new_id.to_hex(),
            ),
        })))
    }
}
