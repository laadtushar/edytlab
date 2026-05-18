//! `silence_region` — zero out audio samples between `start_sec` and
//! `end_sec` on a track, then append a new session node.

use serde::Deserialize;
use serde_json::Value;

use crate::schema::anthropic_tool;
use crate::tool::util::{check_track_index, destructive_edit, load_head_state};
use crate::{Tool, ToolContext, ToolResult};

/// Zero every sample in `[start_sec, end_sec)` of `samples`.
///
/// `channels` is the interleave stride (1 for mono, 2 for stereo, …).
/// Indices that fall outside the buffer are clamped silently.
pub(crate) fn apply_silence(
    samples: &mut [f32],
    sr: u32,
    channels: usize,
    start_sec: f64,
    end_sec: f64,
) {
    let stride = channels.max(1);
    let start = ((start_sec * sr as f64) as usize * stride).min(samples.len());
    let end = ((end_sec * sr as f64) as usize * stride).min(samples.len());
    for s in &mut samples[start..end] {
        *s = 0.0;
    }
}

#[derive(Debug, Deserialize)]
struct Args {
    track: usize,
    start_sec: f64,
    end_sec: f64,
}

pub struct SilenceRegionTool;

impl Tool for SilenceRegionTool {
    fn name(&self) -> &'static str {
        "silence_region"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "silence_region",
            "Zero out audio samples between start_sec and end_sec on a track. \
             Appends a new session node parented to the current head.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "track": {
                        "type": "integer",
                        "description": "Zero-based track index"
                    },
                    "start_sec": {
                        "type": "number",
                        "description": "Start of silence region in seconds (inclusive)"
                    },
                    "end_sec": {
                        "type": "number",
                        "description": "End of silence region in seconds (exclusive)"
                    }
                },
                "required": ["track", "start_sec", "end_sec"],
                "additionalProperties": false
            }),
        )
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };

        if args.start_sec >= args.end_sec {
            return Ok(ToolResult::Error(format!(
                "start_sec ({}) must be < end_sec ({})",
                args.start_sec, args.end_sec
            )));
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

        let start_sec = args.start_sec;
        let end_sec = args.end_sec;

        Ok(destructive_edit(
            ctx,
            args.track,
            move |samples, sr| {
                apply_silence(samples, sr, channels, start_sec, end_sec);
            },
            format!(
                "silence_region track {} {:.3}s..{:.3}s",
                args.track, args.start_sec, args.end_sec
            ),
        ))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::apply_silence;

    #[test]
    fn zeros_samples_in_range() {
        let mut samples = vec![1.0f32; 1000];
        // sr=100, silence 2.0..5.0 sec = frames 200..500, interleaved ch=1
        apply_silence(&mut samples, 100, 1, 2.0, 5.0);
        assert!(
            samples[..200].iter().all(|&s| s == 1.0),
            "before range untouched"
        );
        assert!(
            samples[200..500].iter().all(|&s| s == 0.0),
            "range zeroed"
        );
        assert!(
            samples[500..].iter().all(|&s| s == 1.0),
            "after range untouched"
        );
    }

    #[test]
    fn clamps_to_buffer_end() {
        let mut samples = vec![1.0f32; 100];
        apply_silence(&mut samples, 100, 1, 0.5, 999.0);
        assert!(samples[50..].iter().all(|&s| s == 0.0));
        assert!(samples[..50].iter().all(|&s| s == 1.0));
    }
}
