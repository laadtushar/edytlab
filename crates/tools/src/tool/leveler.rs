use crate::schema::anthropic_tool;
use crate::tool::util::{check_optional_seconds_order, destructive_edit};
use crate::{Tool, ToolContext, ToolResult};
use serde::Deserialize;
use serde_json::Value;

pub(crate) fn apply_leveler(
    samples: &mut [f32],
    sr: u32,
    channels: usize,
    target_db: f32,
    window_ms: u32,
) {
    let channels = channels.max(1);
    let target_rms = 10.0f32.powf(target_db / 20.0);
    let window_frames = ((window_ms as f32 * 0.001 * sr as f32) as usize).max(1);
    let n_frames = samples.len() / channels;
    let mut frame = 0;
    while frame < n_frames {
        let end = (frame + window_frames).min(n_frames);
        let rms: f32 = {
            let slice_start = frame * channels;
            let slice_end = end * channels;
            let sum_sq: f32 = samples[slice_start..slice_end].iter().map(|s| s * s).sum();
            (sum_sq / (slice_end - slice_start) as f32).sqrt()
        };
        if rms > 1e-6 {
            let gain = (target_rms / rms).min(10.0);
            for s in &mut samples[frame * channels..end * channels] {
                *s *= gain;
            }
        }
        frame = end;
    }
}

#[derive(Debug, Deserialize)]
struct Args {
    track: usize,
    target_db: f32,
    start_sec: Option<f64>,
    end_sec: Option<f64>,
}

pub struct LevelerTool;

impl Tool for LevelerTool {
    fn name(&self) -> &'static str {
        "leveler"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "leveler",
            "Apply dynamic leveling: normalise each short window to a target RMS level. Reduces variation between loud and quiet passages. Appends a new session node.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "track": { "type": "integer" },
                    "target_db": { "type": "number", "description": "Target RMS level in dBFS (e.g. -18)" },
                    "start_sec": { "type": "number" },
                    "end_sec": { "type": "number" }
                },
                "required": ["track", "target_db"]
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
        let (target, s, e) = (args.target_db, args.start_sec, args.end_sec);
        Ok(destructive_edit(
            ctx,
            args.track,
            move |samples, sr| {
                let ch = channels;
                let len_frames = samples.len() / ch.max(1);
                let start = s
                    .map(|sec| ((sec * sr as f64) as usize).min(len_frames))
                    .unwrap_or(0);
                let end = e
                    .map(|sec| ((sec * sr as f64) as usize).min(len_frames))
                    .unwrap_or(len_frames);
                apply_leveler(
                    &mut samples[start * ch.max(1)..end * ch.max(1)],
                    sr,
                    ch,
                    target,
                    100,
                );
            },
            format!("leveler track {} target={}dB", args.track, target),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::apply_leveler;

    #[test]
    fn boosts_quiet_section() {
        let mut samples: Vec<f32> = (0..200)
            .map(|i| if i < 100 { 0.1f32 } else { 0.9 })
            .collect();
        apply_leveler(&mut samples, 44100, 1, -12.0, 1);
        let quiet_avg: f32 = samples[..100].iter().map(|s| s.abs()).sum::<f32>() / 100.0;
        assert!(quiet_avg > 0.15, "quiet section boosted, got {quiet_avg}");
    }
}
