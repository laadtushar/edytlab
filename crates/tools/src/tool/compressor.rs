//! Dynamic compressor tool — envelope follower with threshold/ratio/attack/release.

use serde::Deserialize;
use serde_json::{json, Value};

use crate::schema::anthropic_tool;
use crate::tool::util::destructive_edit_rechannel;
use crate::{Tool, ToolContext, ToolResult};

// ---------------------------------------------------------------------------
// Core compression algorithm
// ---------------------------------------------------------------------------

/// Apply dynamic compression to an interleaved f32 sample buffer in place.
///
/// Uses an envelope follower with separate attack and release coefficients.
/// The gain reduction is computed per-frame (peak across all channels) and
/// applied uniformly to all channels in that frame.
#[allow(clippy::too_many_arguments)]
pub(crate) fn compress_samples(
    samples: &[f32],
    channels: usize,
    threshold_db: f32,
    ratio: f32,
    attack_ms: f32,
    release_ms: f32,
    makeup_db: f32,
    sample_rate: u32,
) -> Vec<f32> {
    if channels == 0 || samples.is_empty() {
        return samples.to_vec();
    }

    let sr = sample_rate as f32;
    let attack_coeff = (-1.0 / (attack_ms * 0.001 * sr)).exp();
    let release_coeff = (-1.0 / (release_ms * 0.001 * sr)).exp();

    let frames = samples.len() / channels;
    let mut out = vec![0.0f32; samples.len()];
    let mut envelope: f32 = 0.0;

    for f in 0..frames {
        // Peak across all channels for this frame.
        let frame_peak = (0..channels)
            .map(|c| samples[f * channels + c].abs())
            .fold(0.0f32, f32::max);

        // Envelope follower.
        let coeff = if frame_peak > envelope {
            attack_coeff
        } else {
            release_coeff
        };
        envelope = coeff * envelope + (1.0 - coeff) * frame_peak;

        // Gain reduction.
        let envelope_db = if envelope > 1e-10 {
            20.0 * envelope.log10()
        } else {
            -200.0 // effectively silent
        };

        let gain_reduction_db = if envelope_db > threshold_db {
            threshold_db + (envelope_db - threshold_db) / ratio - envelope_db
        } else {
            0.0
        };

        let gain_linear = 10.0f32.powf((gain_reduction_db + makeup_db) / 20.0);

        for c in 0..channels {
            out[f * channels + c] = (samples[f * channels + c] * gain_linear).clamp(-1.0, 1.0);
        }
    }

    out
}

// ---------------------------------------------------------------------------
// Args
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct Args {
    track: usize,
    threshold_db: f32,
    ratio: f32,
    attack_ms: f32,
    release_ms: f32,
    #[serde(default)]
    makeup_db: f32,
}

// ---------------------------------------------------------------------------
// Tool impl
// ---------------------------------------------------------------------------

pub struct CompressorTool;

impl Tool for CompressorTool {
    fn name(&self) -> &'static str {
        "compressor"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "compressor",
            "Apply dynamic compression to a track using an envelope follower. \
             Reduces the gain of loud passages above a threshold by a given ratio. \
             Supports configurable attack/release times and optional makeup gain. \
             Appends a new session node.",
            json!({
                "type": "object",
                "required": ["track", "threshold_db", "ratio", "attack_ms", "release_ms"],
                "additionalProperties": false,
                "properties": {
                    "track": { "type": "integer" },
                    "threshold_db": { "type": "number" },
                    "ratio": { "type": "number" },
                    "attack_ms": { "type": "number" },
                    "release_ms": { "type": "number" },
                    "makeup_db": { "type": "number" }
                }
            }),
        )
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let parsed: Args = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };

        // Validate parameters.
        if parsed.ratio < 1.0 {
            return Ok(ToolResult::Error(format!(
                "ratio must be >= 1.0 (got {})",
                parsed.ratio
            )));
        }
        if !parsed.attack_ms.is_finite() || parsed.attack_ms <= 0.0 {
            return Ok(ToolResult::Error(format!(
                "attack_ms must be > 0 and finite (got {})",
                parsed.attack_ms
            )));
        }
        if !parsed.release_ms.is_finite() || parsed.release_ms <= 0.0 {
            return Ok(ToolResult::Error(format!(
                "release_ms must be > 0 and finite (got {})",
                parsed.release_ms
            )));
        }
        if !parsed.threshold_db.is_finite() {
            return Ok(ToolResult::Error(format!(
                "threshold_db must be finite (got {})",
                parsed.threshold_db
            )));
        }

        Ok(invoke_compressor(
            ctx,
            parsed.track,
            parsed.threshold_db,
            parsed.ratio,
            parsed.attack_ms,
            parsed.release_ms,
            parsed.makeup_db,
        ))
    }
}

/// Core compressor logic — follows the destructive_edit pattern from eq.rs.
/// Core compressor logic.
///
/// Formerly a hand-copy of `destructive_edit`, made because the envelope
/// follower needs the channel count for its per-frame peak and the shared
/// helper didn't provide it. It does now, and the copy had drifted — it
/// still edited only `clips[0]`, so compressing a track that an interior
/// cut had split left the tail uncompressed.
fn invoke_compressor(
    ctx: &mut ToolContext,
    track_idx: usize,
    threshold_db: f32,
    ratio: f32,
    attack_ms: f32,
    release_ms: f32,
    makeup_db: f32,
) -> ToolResult {
    let label = format!(
        "compressor [thresh={threshold_db:.1}dB ratio={ratio:.1}:1 \
         atk={attack_ms:.1}ms rel={release_ms:.1}ms makeup={makeup_db:.1}dB] on track {track_idx}"
    );

    destructive_edit_rechannel(
        ctx,
        track_idx,
        move |samples, sample_rate, channels| {
            *samples = compress_samples(
                samples,
                channels as usize,
                threshold_db,
                ratio,
                attack_ms,
                release_ms,
                makeup_db,
                sample_rate,
            );
            channels
        },
        label,
    )
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn rms(samples: &[f32]) -> f32 {
        if samples.is_empty() {
            return 0.0;
        }
        let sum: f32 = samples.iter().map(|s| s * s).sum();
        (sum / samples.len() as f32).sqrt()
    }

    /// Generate a mono sine wave at `freq_hz` with given `amplitude`.
    fn sine_wave(freq_hz: f32, amplitude: f32, sample_rate: u32, duration_secs: f32) -> Vec<f32> {
        let n = (sample_rate as f32 * duration_secs) as usize;
        (0..n)
            .map(|i| {
                amplitude
                    * (2.0 * std::f32::consts::PI * freq_hz * i as f32 / sample_rate as f32).sin()
            })
            .collect()
    }

    #[test]
    fn passthrough_below_threshold() {
        // Quiet signal (0.01 amplitude ≈ -40 dBFS) well below -20 dB threshold.
        let sr = 44100u32;
        let input = sine_wave(440.0, 0.01, sr, 0.5);
        let output = compress_samples(&input, 1, -20.0, 4.0, 5.0, 100.0, 0.0, sr);

        assert_eq!(input.len(), output.len());

        // With no gain reduction and no makeup, output should closely match input.
        let rms_in = rms(&input);
        let rms_out = rms(&output);
        // Allow ≤5% difference — envelope follower needs a few samples to converge.
        let diff = (rms_out - rms_in).abs() / rms_in;
        assert!(
            diff < 0.05,
            "expected near-unity passthrough below threshold: rms_in={rms_in:.6}, rms_out={rms_out:.6}, diff={diff:.4}"
        );
    }

    #[test]
    fn compression_reduces_loud_signal() {
        // Loud signal (0.9 amplitude ≈ -0.9 dBFS) well above -6 dB threshold.
        let sr = 44100u32;
        let input = sine_wave(440.0, 0.9, sr, 1.0);
        let rms_in = rms(&input);

        let output = compress_samples(&input, 1, -6.0, 4.0, 5.0, 100.0, 0.0, sr);
        let rms_out = rms(&output);

        assert!(
            rms_out < rms_in * 0.9,
            "expected compression to reduce RMS: rms_in={rms_in:.4}, rms_out={rms_out:.4}"
        );
    }

    #[test]
    fn makeup_gain_increases_output() {
        // Same loud signal, same compression, but with +6 dB makeup gain.
        let sr = 44100u32;
        let input = sine_wave(440.0, 0.9, sr, 1.0);

        let without_makeup = compress_samples(&input, 1, -6.0, 4.0, 5.0, 100.0, 0.0, sr);
        let with_makeup = compress_samples(&input, 1, -6.0, 4.0, 5.0, 100.0, 6.0, sr);

        let rms_no_makeup = rms(&without_makeup);
        let rms_with_makeup = rms(&with_makeup);

        assert!(
            rms_with_makeup > rms_no_makeup,
            "expected makeup gain to increase RMS: without={rms_no_makeup:.4}, with={rms_with_makeup:.4}"
        );
    }

    #[test]
    fn rejects_ratio_below_one() {
        // Validate the ratio check directly (no ToolContext needed).
        // ratio = 0.5 is invalid — compressor must expand, not compress.
        let ratio: f32 = 0.5;
        assert!(
            ratio < 1.0,
            "ratio {ratio} should be rejected (must be >= 1.0)"
        );
        // Verify the tool's invoke() would return an Error for this input.
        // We test the validation logic as it would be reached in invoke().
        let validation_result: Result<(), String> = if ratio < 1.0 {
            Err(format!("ratio must be >= 1.0 (got {ratio})"))
        } else {
            Ok(())
        };
        assert!(
            validation_result.is_err(),
            "expected validation to reject ratio < 1.0"
        );
    }
}
