use serde::Deserialize;
use serde_json::Value;

use crate::schema::anthropic_tool;
use crate::tool::util::{check_optional_seconds_order, destructive_edit};
use crate::{Tool, ToolContext, ToolResult};

/// L-R center cancellation. Effective on stereo tracks where vocals are panned center.
pub(crate) fn apply_vocal_reduction(samples: &mut [f32], _sr: u32, channels: usize) {
    if channels < 2 {
        return;
    }
    let n_frames = samples.len() / channels;
    for frame in 0..n_frames {
        let l = samples[frame * channels];
        let r = samples[frame * channels + 1];
        let side = (l - r) / 2.0;
        samples[frame * channels] = side;
        samples[frame * channels + 1] = -side;
    }
}

#[derive(Debug, Deserialize)]
struct Args {
    track: usize,
    start_sec: Option<f64>,
    end_sec: Option<f64>,
}

pub struct VocalReductionTool;

impl Tool for VocalReductionTool {
    fn name(&self) -> &'static str {
        "vocal_reduction"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "vocal_reduction",
            "Reduce center-panned vocals using L-R channel subtraction (Karaoke effect). Works on stereo tracks; results depend on how centrally the vocals are mixed. Appends a new session node.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "track": { "type": "integer" },
                    "start_sec": { "type": "number" },
                    "end_sec": { "type": "number" }
                },
                "required": ["track"]
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
        let channels = {
            let state = match crate::tool::util::load_head_state(ctx) {
                Ok(s) => s,
                Err(e) => return Ok(ToolResult::Error(e)),
            };
            if let Err(e) = crate::tool::util::check_track_index(&state.tracks, args.track) {
                return Ok(ToolResult::Error(e));
            }
            let clip = state.tracks[args.track].clips.first().cloned();
            if let Some(c) = clip {
                audio_decoder::decode_file(&c.source_path)
                    .map(|d| d.channels as usize)
                    .unwrap_or(1)
            } else {
                return Ok(ToolResult::Error(format!(
                    "track {} has no clips",
                    args.track
                )));
            }
        };
        if channels < 2 {
            return Ok(ToolResult::Error(
                "vocal_reduction requires a stereo track".into(),
            ));
        }
        let (s, e) = (args.start_sec, args.end_sec);
        Ok(destructive_edit(
            ctx,
            args.track,
            move |samples, sr| {
                let ch = channels;
                let len_frames = samples.len() / ch;
                let start = s
                    .map(|sec| ((sec * sr as f64) as usize).min(len_frames))
                    .unwrap_or(0);
                let end = e
                    .map(|sec| ((sec * sr as f64) as usize).min(len_frames))
                    .unwrap_or(len_frames);
                apply_vocal_reduction(&mut samples[start * ch..end * ch], sr, ch);
            },
            format!("vocal_reduction track {}", args.track),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::apply_vocal_reduction;

    #[test]
    fn center_cancel_reduces_center() {
        let mut samples = vec![1.0f32, 0.6, 1.0, 0.6];
        apply_vocal_reduction(&mut samples, 44100, 2);
        let after_l = samples[0];
        let after_r = samples[1];
        assert!(
            after_l != 1.0 || after_r != 0.6,
            "samples should be modified"
        );
    }
}
