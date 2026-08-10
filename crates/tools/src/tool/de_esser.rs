use serde::Deserialize;
use serde_json::Value;

use crate::schema::anthropic_tool;
use crate::tool::util::{
    biquad_process, check_optional_seconds_order, destructive_edit, BiquadCoeffs,
};
use crate::{Tool, ToolContext, ToolResult};

pub(crate) fn apply_de_esser(
    samples: &mut [f32],
    sr: u32,
    channels: usize,
    frequency_hz: f32,
    threshold_db: f32,
) {
    let channels = channels.max(1);
    let threshold_lin = 10.0f32.powf(threshold_db / 20.0);
    let n_frames = samples.len() / channels;
    let coeffs = BiquadCoeffs::high_pass(frequency_hz, sr);
    let mut detector: Vec<f32> = samples.to_vec();
    biquad_process(&mut detector, channels, &coeffs, 0, n_frames);
    let attack_coeff = (-1.0f32 / (2.0 * 0.001 * sr as f32)).exp();
    let release_coeff = (-1.0f32 / (100.0 * 0.001 * sr as f32)).exp();
    let mut env = 0.0f32;
    for frame in 0..n_frames {
        let peak = (0..channels)
            .map(|ch| detector[frame * channels + ch].abs())
            .fold(0.0f32, f32::max);
        let coeff = if peak > env {
            attack_coeff
        } else {
            release_coeff
        };
        env = peak + coeff * (env - peak);
        if env > threshold_lin {
            let reduction = threshold_lin / env;
            for ch in 0..channels {
                samples[frame * channels + ch] *= reduction;
            }
        }
    }
}

#[derive(Debug, Deserialize)]
struct Args {
    track: usize,
    frequency_hz: Option<f32>,
    threshold_db: f32,
    start_sec: Option<f64>,
    end_sec: Option<f64>,
}

pub struct DeEsserTool;

impl Tool for DeEsserTool {
    fn name(&self) -> &'static str {
        "de_esser"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "de_esser",
            "Reduce harsh sibilant 's' and 'sh' sounds. frequency_hz sets where sibilance detection begins (default 7000Hz); threshold_db is the compression trigger level. Appends a new session node.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "track": { "type": "integer" },
                    "frequency_hz": { "type": "number", "default": 7000.0 },
                    "threshold_db": { "type": "number", "description": "Detection threshold in dBFS (e.g. -20)" },
                    "start_sec": { "type": "number" },
                    "end_sec": { "type": "number" }
                },
                "required": ["track", "threshold_db"]
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
        let freq = args.frequency_hz.unwrap_or(7000.0).max(1000.0);
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
        let (f, t, s, e) = (freq, args.threshold_db, args.start_sec, args.end_sec);
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
                apply_de_esser(
                    &mut samples[start * ch.max(1)..end * ch.max(1)],
                    sr,
                    ch,
                    f,
                    t,
                );
            },
            format!(
                "de_esser track {} freq={:.0}Hz threshold={}dB",
                args.track, freq, args.threshold_db
            ),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::apply_de_esser;

    #[test]
    fn reduces_high_freq_energy() {
        let mut samples: Vec<f32> = (0..44100).map(|i| ((i as f32 * 0.1).sin() * 0.9)).collect();
        let before_max = samples.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        apply_de_esser(&mut samples, 44100, 1, 8000.0, -20.0);
        let after_max = samples.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        assert!(
            after_max <= before_max + 1e-4,
            "de-esser should not amplify"
        );
    }
}
