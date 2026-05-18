use crate::schema::anthropic_tool;
use crate::tool::util::destructive_edit;
use crate::{Tool, ToolContext, ToolResult};
use serde::Deserialize;
use serde_json::Value;

pub(crate) fn apply_stereo_to_mono(samples: &[f32], channels: usize) -> Vec<f32> {
    if channels <= 1 {
        return samples.to_vec();
    }
    let n_frames = samples.len() / channels;
    (0..n_frames)
        .map(|f| {
            (0..channels)
                .map(|ch| samples[f * channels + ch])
                .sum::<f32>()
                / channels as f32
        })
        .collect()
}

#[derive(Debug, Deserialize)]
struct Args {
    track: usize,
}

pub struct StereoToMonoTool;

impl Tool for StereoToMonoTool {
    fn name(&self) -> &'static str {
        "stereo_to_mono"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "stereo_to_mono",
            "Convert a stereo (or multi-channel) track to mono by averaging all channels. Appends a new session node.",
            serde_json::json!({
                "type": "object",
                "properties": { "track": { "type": "integer" } },
                "required": ["track"]
            }),
        )
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };
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
        Ok(destructive_edit(
            ctx,
            args.track,
            move |samples, _sr| {
                let mono = apply_stereo_to_mono(samples, channels);
                *samples = mono;
            },
            format!("stereo_to_mono track {}", args.track),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::apply_stereo_to_mono;

    #[test]
    fn averages_channels() {
        let stereo = vec![0.8f32, 0.4, 0.6, 0.2];
        let mono = apply_stereo_to_mono(&stereo, 2);
        assert_eq!(mono.len(), 2);
        assert!((mono[0] - 0.6).abs() < 1e-5);
        assert!((mono[1] - 0.4).abs() < 1e-5);
    }
}
