//! `normalize_loudness` — set a track's gain so its integrated loudness
//! hits a LUFS target.
//!
//! `normalize` is peak-based, and every real delivery target is
//! loudness: −14 LUFS (Spotify, YouTube), −16 (Apple Podcasts), −23
//! (EBU R128 broadcast). Two files peak-normalised to the same value
//! can differ by 10 LUFS, so peak normalisation cannot answer "make
//! this as loud as everything else" — which is the question people
//! actually have.
//!
//! The measurement is not new: `audio-analysis::loudness` already
//! implements EBU R128 via the pure-Rust `ebur128` crate, and
//! `analyze_track` already reports it. This is a gain calculation on
//! top of a tested measurement.
//!
//! ## The clipping decision
//!
//! Reaching −14 LUFS routinely needs enough gain to push peaks past
//! full scale. Applying it anyway and letting the render clip would be
//! a tool reporting success while wrecking the audio — the same class
//! of failure as a filter diverging past Nyquist.
//!
//! So the gain is **capped** at whatever keeps the peak under
//! `true_peak_ceiling_db`, and the result reports the shortfall: what
//! was asked for, what was applied, and the loudness actually achieved.
//! The alternative — applying full gain and limiting — hides a second
//! processing stage inside a "normalise" verb, and changes the shape of
//! the signal rather than just its level. A caller who wants that can
//! run `limiter` afterwards, having been told it is needed.
//!
//! Like `normalize`, this is non-destructive: it sets `Track::gain_db`
//! and the engine applies it at render time, so undo is free and the
//! source file is untouched.

use serde::Deserialize;
use serde_json::{json, Value};

use crate::schema::anthropic_tool;
use crate::tool::util::{
    append_state, check_track_index, flatten_track, load_head_state, TrackAudio,
};
use crate::{Tool, ToolContext, ToolResult};

/// Below this the measurement is meaningless and the gain to reach a
/// normal target would be absurd. `ebur128` reports roughly -70 LUFS
/// for digital silence.
const MIN_MEASURABLE_LUFS: f32 = -70.0;

fn default_ceiling() -> f32 {
    -1.0
}

#[derive(Debug, Deserialize)]
struct Args {
    track: usize,
    target_lufs: f32,
    #[serde(default = "default_ceiling")]
    true_peak_ceiling_db: f32,
}

pub struct NormalizeLoudnessTool;

impl Tool for NormalizeLoudnessTool {
    fn name(&self) -> &'static str {
        "normalize_loudness"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "normalize_loudness",
            "Set a track's gain so its integrated loudness (EBU R128) reaches target_lufs. \
             Use this rather than `normalize` for delivery: -14 LUFS for Spotify and YouTube, \
             -16 for Apple Podcasts, -23 for broadcast. Peak normalisation cannot match \
             perceived loudness between files. \
             If the gain needed would push peaks above true_peak_ceiling_db (default -1 dBFS), \
             it is capped there instead of clipping, and the result reports the shortfall in \
             `achieved_lufs` and `shortfall_db` — run `limiter` first if you need to close it. \
             The source file is not rewritten; the engine applies the gain at render time.",
            json!({
                "type": "object",
                "properties": {
                    "track": { "type": "integer" },
                    "target_lufs": {
                        "type": "number",
                        "description": "e.g. -14 for streaming, -23 for broadcast"
                    },
                    "true_peak_ceiling_db": {
                        "type": "number",
                        "description": "Peak ceiling in dBFS; gain is capped to respect it. Default -1.0"
                    },
                },
                "required": ["track", "target_lufs"],
                "additionalProperties": false,
            }),
        )
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };

        if !args.target_lufs.is_finite() {
            return Ok(ToolResult::Error(format!(
                "target_lufs must be finite; got {}",
                args.target_lufs
            )));
        }
        // LUFS targets are negative by construction: 0 LUFS is full-scale
        // pink noise, and nothing is delivered above it.
        if args.target_lufs > 0.0 {
            return Ok(ToolResult::Error(format!(
                "target_lufs must be <= 0; got {}. Common targets are -14 \
                 (streaming), -16 (podcasts), -23 (broadcast)",
                args.target_lufs
            )));
        }
        if args.target_lufs < MIN_MEASURABLE_LUFS {
            return Ok(ToolResult::Error(format!(
                "target_lufs {} is below the measurable floor ({MIN_MEASURABLE_LUFS} LUFS)",
                args.target_lufs
            )));
        }
        if !args.true_peak_ceiling_db.is_finite() || args.true_peak_ceiling_db > 0.0 {
            return Ok(ToolResult::Error(format!(
                "true_peak_ceiling_db must be finite and <= 0; got {}",
                args.true_peak_ceiling_db
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
        if track.clips.is_empty() {
            return Ok(ToolResult::Error(
                "track has no clips; nothing to normalize".into(),
            ));
        }

        // The whole timeline, not `clips[0]`'s source file. After a cut
        // that file still holds the audio the cut removed, and the gain
        // lands on the whole track — so measuring the head alone would
        // set a level from material the listener never hears.
        let TrackAudio {
            window,
            sample_rate,
            channels,
        } = match flatten_track(&track.clips) {
            Ok(a) => a,
            Err(msg) => return Ok(ToolResult::Error(msg)),
        };

        let measured =
            match audio_analysis::loudness::integrated_lufs(&window, sample_rate, channels) {
                Ok(l) => l,
                Err(e) => {
                    return Ok(ToolResult::Error(format!(
                        "loudness measurement failed: {e}"
                    )))
                }
            };

        if !measured.is_finite() || measured < MIN_MEASURABLE_LUFS {
            return Ok(ToolResult::Error(format!(
                "track measures {measured:.1} LUFS, at or below the silence \
                 floor; there is nothing to normalise"
            )));
        }

        let requested_gain_db = args.target_lufs - measured;

        // What the peak becomes after that gain, and how much headroom
        // the ceiling actually leaves.
        let peak = window.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        if peak <= 0.0 {
            return Ok(ToolResult::Error(
                "track is silent; cannot normalize".into(),
            ));
        }
        let peak_dbfs = 20.0 * peak.log10();
        let max_gain_db = args.true_peak_ceiling_db - peak_dbfs;

        let applied_gain_db = requested_gain_db.min(max_gain_db);
        let capped = applied_gain_db < requested_gain_db;
        // Loudness moves dB-for-dB with gain, so the achieved value is
        // the measurement plus whatever gain survived the cap.
        let achieved_lufs = measured + applied_gain_db;
        let shortfall_db = args.target_lufs - achieved_lufs;

        // Absolute, not compositional: the caller asked for a level, not
        // for a nudge relative to whatever gain is already set.
        track.gain_db = applied_gain_db;

        let new_id = match append_state(
            ctx,
            state,
            format!(
                "normalize_loudness track {} -> {} LUFS",
                args.track, args.target_lufs
            ),
        ) {
            Ok(id) => id,
            Err(msg) => return Ok(ToolResult::Error(msg)),
        };

        let summary = if capped {
            format!(
                "Track {} measured {:.1} LUFS. Reaching {:.1} needed {:+.1} dB, but that \
                 would peak above {:.1} dBFS, so {:+.1} dB was applied instead — \
                 {:.1} LUFS, {:.1} dB short. Run limiter first to close the gap.",
                args.track,
                measured,
                args.target_lufs,
                requested_gain_db,
                args.true_peak_ceiling_db,
                applied_gain_db,
                achieved_lufs,
                shortfall_db,
            )
        } else {
            format!(
                "Track {} measured {:.1} LUFS; applied {:+.1} dB to reach {:.1} LUFS.",
                args.track, measured, applied_gain_db, achieved_lufs,
            )
        };

        Ok(ToolResult::Ok(json!({
            "node_id": new_id.to_hex(),
            "measured_lufs": measured,
            "target_lufs": args.target_lufs,
            "requested_gain_db": requested_gain_db,
            "applied_gain_db": applied_gain_db,
            "achieved_lufs": achieved_lufs,
            "shortfall_db": shortfall_db,
            "capped_by_ceiling": capped,
            "true_peak_ceiling_db": args.true_peak_ceiling_db,
            "summary": summary,
        })))
    }
}
