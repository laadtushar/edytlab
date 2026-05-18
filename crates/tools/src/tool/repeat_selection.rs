//! `repeat_selection` — duplicate a region N times, appending copies
//! after the end of the buffer.

use serde::Deserialize;
use serde_json::Value;

use crate::schema::anthropic_tool;
use crate::tool::util::{check_track_index, destructive_edit, load_head_state};
use crate::{Tool, ToolContext, ToolResult};

/// Appends `times` copies of the region `[start_sec, end_sec)` to the
/// end of `samples`. The original samples are left untouched.
///
/// `channels` is the interleave stride (1 for mono, 2 for stereo, …).
/// Indices that fall outside the buffer are clamped silently.
pub(crate) fn apply_repeat(
    samples: &mut Vec<f32>,
    sr: u32,
    channels: usize,
    start_sec: f64,
    end_sec: f64,
    times: u32,
) {
    let stride = channels.max(1);
    let start = ((start_sec * sr as f64) as usize * stride).min(samples.len());
    let end = ((end_sec * sr as f64) as usize * stride).min(samples.len());
    if end <= start || times == 0 {
        return;
    }
    let region: Vec<f32> = samples[start..end].to_vec();
    for _ in 0..times {
        samples.extend_from_slice(&region);
    }
}

#[derive(Debug, Deserialize)]
struct Args {
    track: usize,
    start_sec: f64,
    end_sec: f64,
    times: u32,
}

pub struct RepeatSelectionTool;

impl Tool for RepeatSelectionTool {
    fn name(&self) -> &'static str {
        "repeat_selection"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "repeat_selection",
            "Duplicate the audio region [start_sec, end_sec) on a track N additional times, \
             appending copies after the original buffer. Appends a new session node.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "track": {
                        "type": "integer",
                        "description": "Zero-based track index"
                    },
                    "start_sec": {
                        "type": "number",
                        "description": "Start of region to repeat in seconds (inclusive)"
                    },
                    "end_sec": {
                        "type": "number",
                        "description": "End of region to repeat in seconds (exclusive)"
                    },
                    "times": {
                        "type": "integer",
                        "minimum": 1,
                        "description": "Number of additional copies to append"
                    }
                },
                "required": ["track", "start_sec", "end_sec", "times"],
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
            return Ok(ToolResult::Error(
                "start_sec must be < end_sec".into(),
            ));
        }

        // Pre-read channel count so the closure can compute the correct
        // interleaved byte range.
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

        let (s, e, t) = (args.start_sec, args.end_sec, args.times);
        Ok(destructive_edit(
            ctx,
            args.track,
            move |samples, sr| apply_repeat(samples, sr, channels, s, e, t),
            format!(
                "repeat_selection track {} {:.3}s..{:.3}s x{}",
                args.track, args.start_sec, args.end_sec, args.times
            ),
        ))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::apply_repeat;

    #[test]
    fn repeats_twice() {
        let mut samples: Vec<f32> = (0..10).map(|i| i as f32).collect();
        // sr=10, region 3.0..7.0 sec = frames 30..70 but buffer is only
        // 10 samples long, so clamped to indices 3..7 (channels=1).
        apply_repeat(&mut samples, 10, 1, 0.3, 0.7, 2);
        assert_eq!(samples.len(), 18);
        // Original is untouched.
        assert_eq!(&samples[0..10], &[0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0]);
        // First copy appended.
        assert_eq!(&samples[10..14], &[3.0, 4.0, 5.0, 6.0]);
        // Second copy appended.
        assert_eq!(&samples[14..18], &[3.0, 4.0, 5.0, 6.0]);
    }

    #[test]
    fn noop_when_times_is_zero() {
        let mut samples: Vec<f32> = vec![1.0; 10];
        apply_repeat(&mut samples, 10, 1, 0.0, 0.5, 0);
        assert_eq!(samples.len(), 10);
    }

    #[test]
    fn noop_when_region_is_empty() {
        let mut samples: Vec<f32> = vec![1.0; 10];
        apply_repeat(&mut samples, 10, 1, 0.5, 0.5, 3);
        assert_eq!(samples.len(), 10);
    }
}
