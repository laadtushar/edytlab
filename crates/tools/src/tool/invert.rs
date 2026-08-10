//! `invert` — negate all samples (optionally within a time range) on a track,
//! then append a new session node.

use serde::Deserialize;
use serde_json::Value;

use crate::schema::anthropic_tool;
use crate::tool::util::{
    check_optional_seconds_order, check_track_index, destructive_edit, load_head_state,
};
use crate::{Tool, ToolContext, ToolResult};

/// Negate every sample in `[start_sec, end_sec)` of `samples` (or all samples
/// when both bounds are `None`).
///
/// `channels` is the interleave stride (1 for mono, 2 for stereo, …).
/// Indices that fall outside the buffer are clamped silently.
pub(crate) fn apply_invert(
    samples: &mut [f32],
    sr: u32,
    channels: usize,
    start_sec: Option<f64>,
    end_sec: Option<f64>,
) {
    let stride = channels.max(1);
    let start = start_sec
        .map(|s| ((s * sr as f64) as usize * stride).min(samples.len()))
        .unwrap_or(0);
    let end = end_sec
        .map(|e| ((e * sr as f64) as usize * stride).min(samples.len()))
        .unwrap_or(samples.len());
    for s in &mut samples[start..end] {
        *s = -*s;
    }
}

#[derive(Debug, Deserialize)]
struct Args {
    track: usize,
    start_sec: Option<f64>,
    end_sec: Option<f64>,
}

pub struct InvertTool;

impl Tool for InvertTool {
    fn name(&self) -> &'static str {
        "invert"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "invert",
            "Invert (negate) audio polarity on a track, optionally within a time range. \
             Appends a new session node.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "track": {
                        "type": "integer",
                        "description": "Zero-based track index"
                    },
                    "start_sec": {
                        "type": "number",
                        "description": "Start of invert region in seconds (inclusive). Omit to start at 0."
                    },
                    "end_sec": {
                        "type": "number",
                        "description": "End of invert region in seconds (exclusive). Omit to invert to end of track."
                    }
                },
                "required": ["track"],
                "additionalProperties": false
            }),
        )
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };

        // A reversed window would survive independent clamping and
        // panic on the slice below.
        if let Err(e) = check_optional_seconds_order(args.start_sec, args.end_sec) {
            return Ok(ToolResult::Error(e));
        }

        // Pre-read channel count so the closure (which only receives sr)
        // can compute the correct interleaved byte range.
        let channels = {
            let state = match load_head_state(ctx) {
                Ok(s) => s,
                Err(e) => return Ok(ToolResult::Error(e)),
            };
            if let Err(e) = check_track_index(&state.tracks, args.track) {
                return Ok(ToolResult::Error(e));
            }
            let clip = match state.tracks[args.track].clips.first().cloned() {
                Some(c) => c,
                None => {
                    return Ok(ToolResult::Error(format!(
                        "track {} has no clips",
                        args.track
                    )))
                }
            };
            match audio_decoder::decode_file(&clip.source_path) {
                Ok(d) => d.channels as usize,
                Err(_) => 1,
            }
        };

        let (s, e) = (args.start_sec, args.end_sec);

        Ok(destructive_edit(
            ctx,
            args.track,
            move |samples, sr| apply_invert(samples, sr, channels, s, e),
            format!("invert track {}", args.track),
        ))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::apply_invert;

    #[test]
    fn negates_all() {
        let mut s = vec![0.5f32, -0.3, 0.0, 1.0];
        apply_invert(&mut s, 44100, 1, None, None);
        assert!((s[0] - -0.5).abs() < 1e-6);
        assert!((s[1] - 0.3).abs() < 1e-6);
        assert_eq!(s[2], 0.0);
        assert!((s[3] - -1.0).abs() < 1e-6);
    }

    #[test]
    fn negates_range_only() {
        let mut s = vec![1.0f32; 200]; // sr=100, 2sec, ch=1
        apply_invert(&mut s, 100, 1, Some(0.5), Some(1.5));
        // frames 50..150 negated
        assert_eq!(s[0], 1.0);
        assert_eq!(s[49], 1.0);
        assert_eq!(s[50], -1.0);
        assert_eq!(s[149], -1.0);
        assert_eq!(s[150], 1.0);
    }
}
