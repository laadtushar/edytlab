//! Spectral noise-reduction tool — spectral subtraction with overlap-add.

use std::f32::consts::PI;

use realfft::RealFftPlanner;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::schema::anthropic_tool;
use crate::tool::util::destructive_edit_rechannel;
use crate::{Tool, ToolContext, ToolResult};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const FFT_SIZE: usize = 2048;
const HOP_SIZE: usize = 512; // 75 % overlap

// ---------------------------------------------------------------------------
// Core DSP — pub(crate) so unit tests can reach it without a ToolContext
// ---------------------------------------------------------------------------

/// Process a single mono channel through spectral subtraction (overlap-add).
///
/// * `mono`              – mono samples (one sample per frame)
/// * `noise_duration_sec` – seconds at the start of the clip that contain
///   only noise (used to build the noise profile)
/// * `strength` – how aggressively to subtract (0.0 … 1.0)
/// * `floor` – spectral floor as a fraction of the original
///   magnitude (prevents musical-noise artefacts)
/// * `sample_rate`       – samples per second (for converting seconds → frames)
pub(crate) fn process_channel(
    mono: &[f32],
    noise_duration_sec: f32,
    strength: f32,
    floor: f32,
    sample_rate: u32,
) -> Vec<f32> {
    if mono.is_empty() {
        return Vec::new();
    }

    let n = mono.len();

    // --- Build Hann window --------------------------------------------------
    let window: Vec<f32> = (0..FFT_SIZE)
        .map(|i| 0.5 * (1.0 - (2.0 * PI * i as f32 / FFT_SIZE as f32).cos()))
        .collect();

    // --- FFT planner --------------------------------------------------------
    let mut planner = RealFftPlanner::<f32>::new();
    let r2c = planner.plan_fft_forward(FFT_SIZE);
    let c2r = planner.plan_fft_inverse(FFT_SIZE);
    let num_bins = FFT_SIZE / 2 + 1;

    // --- Build noise profile ------------------------------------------------
    let profile_samples = (noise_duration_sec * sample_rate as f32).round() as usize;
    let profile_samples = profile_samples.min(n);

    let mut noise_profile = vec![0.0f32; num_bins];
    let mut profile_frame_count = 0usize;

    let mut frame_buf = vec![0.0f32; FFT_SIZE];
    let mut spectrum = r2c.make_output_vec();

    let mut pos = 0usize;
    while pos + FFT_SIZE <= profile_samples.max(FFT_SIZE) && pos + FFT_SIZE <= n {
        for (i, fb) in frame_buf.iter_mut().enumerate() {
            *fb = if pos + i < n { mono[pos + i] } else { 0.0 } * window[i];
        }
        r2c.process(&mut frame_buf, &mut spectrum).unwrap();
        for (bin, s) in spectrum.iter().enumerate() {
            noise_profile[bin] += s.norm();
        }
        profile_frame_count += 1;
        pos += HOP_SIZE;
        if pos >= profile_samples {
            break;
        }
    }

    if profile_frame_count > 0 {
        let count = profile_frame_count as f32;
        for v in &mut noise_profile {
            *v /= count;
        }
    }

    // --- Overlap-add output buffers -----------------------------------------
    let out_len = n + FFT_SIZE; // generous; we truncate to n at the end
    let mut output = vec![0.0f32; out_len];
    let mut norm = vec![0.0f32; out_len];

    // --- STFT subtraction loop ----------------------------------------------
    let mut ifft_buf = vec![0.0f32; FFT_SIZE];

    pos = 0;
    loop {
        if pos >= n {
            break;
        }
        // Fill frame with zero-padding at the end of the signal.
        for (i, fb) in frame_buf.iter_mut().enumerate() {
            *fb = if pos + i < n { mono[pos + i] } else { 0.0 } * window[i];
        }

        // Forward FFT.
        r2c.process(&mut frame_buf, &mut spectrum).unwrap();

        // Spectral subtraction.
        for (bin, s) in spectrum.iter_mut().enumerate() {
            let mag = s.norm();
            let phase = s.arg();
            let reduced = (mag - noise_profile[bin] * strength).max(mag * floor);
            *s = realfft::num_complex::Complex::from_polar(reduced, phase);
        }

        // realfft requires DC (bin 0) and Nyquist (last bin) to be purely real —
        // enforce this after the magnitude-preserving phase reconstruction above
        // since floating-point operations may leave tiny imaginary residues.
        spectrum[0].im = 0.0;
        let last = num_bins - 1;
        spectrum[last].im = 0.0;

        // Inverse FFT.
        c2r.process(&mut spectrum, &mut ifft_buf).unwrap();

        // Normalise by FFT_SIZE (realfft does not normalise inverse).
        let scale = 1.0 / FFT_SIZE as f32;
        for (i, v) in ifft_buf.iter().enumerate() {
            let idx = pos + i;
            if idx < out_len {
                output[idx] += v * scale * window[i];
                norm[idx] += window[i] * window[i];
            }
        }

        pos += HOP_SIZE;
    }

    // --- Divide by overlap-add normalisation factor -------------------------
    for (o, nm) in output.iter_mut().zip(norm.iter()) {
        if *nm > 1e-10 {
            *o /= nm;
        }
    }

    output.truncate(n);
    output
}

// ---------------------------------------------------------------------------
// Args
// ---------------------------------------------------------------------------

fn default_strength() -> f32 {
    0.85
}

fn default_floor() -> f32 {
    0.05
}

#[derive(Debug, Deserialize)]
struct Args {
    track: usize,
    noise_duration_sec: f32,
    #[serde(default = "default_strength")]
    strength: f32,
    #[serde(default = "default_floor")]
    floor: f32,
}

// ---------------------------------------------------------------------------
// Tool impl
// ---------------------------------------------------------------------------

pub struct NoiseReductionTool;

impl Tool for NoiseReductionTool {
    fn name(&self) -> &'static str {
        "noise_reduction"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "noise_reduction",
            "Apply spectral noise reduction (spectral subtraction, overlap-add) to a track. \
             The first `noise_duration_sec` seconds of the clip are used as the noise profile. \
             `strength` controls how aggressively noise is subtracted (default 0.85); \
             `floor` sets the minimum fraction of the original magnitude retained to avoid \
             musical-noise artefacts (default 0.05). Appends a new session node.",
            json!({
                "type": "object",
                "required": ["track", "noise_duration_sec"],
                "additionalProperties": false,
                "properties": {
                    "track": { "type": "integer" },
                    "noise_duration_sec": { "type": "number" },
                    "strength": { "type": "number" },
                    "floor": { "type": "number" }
                }
            }),
        )
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let parsed: Args = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };

        if !parsed.noise_duration_sec.is_finite() || parsed.noise_duration_sec <= 0.0 {
            return Ok(ToolResult::Error(
                "noise_duration_sec must be > 0 and finite".to_string(),
            ));
        }
        if !(0.0..=1.0).contains(&parsed.strength) {
            return Ok(ToolResult::Error(format!(
                "strength must be in [0.0, 1.0] (got {})",
                parsed.strength
            )));
        }
        if !(0.0..=1.0).contains(&parsed.floor) {
            return Ok(ToolResult::Error(format!(
                "floor must be in [0.0, 1.0] (got {})",
                parsed.floor
            )));
        }

        Ok(invoke_noise_reduction(
            ctx,
            parsed.track,
            parsed.noise_duration_sec,
            parsed.strength,
            parsed.floor,
        ))
    }
}

/// Core noise-reduction logic.
///
/// Formerly a hand-copy of `destructive_edit`, made because the spectral
/// subtraction runs per channel and the shared helper didn't hand over
/// the channel count. It does now, and the copy had drifted — it still
/// edited only `clips[0]`, so a track split by an interior cut kept its
/// noise floor on everything after the cut.
fn invoke_noise_reduction(
    ctx: &mut ToolContext,
    track_idx: usize,
    noise_duration_sec: f32,
    strength: f32,
    floor: f32,
) -> ToolResult {
    let label = format!(
        "noise_reduction (profile={noise_duration_sec:.2}s, strength={strength:.2}, floor={floor:.2}) on track {track_idx}"
    );

    destructive_edit_rechannel(
        ctx,
        track_idx,
        move |samples, sample_rate, channels| {
            let channels = channels.max(1) as usize;
            let num_frames = samples.len() / channels;
            let mut result = vec![0.0f32; samples.len()];
            // Spectral subtraction runs per channel, so the interleaved
            // buffer is split apart and put back together around it.
            for ch in 0..channels {
                let mono: Vec<f32> = (0..num_frames)
                    .map(|f| samples[f * channels + ch])
                    .collect();
                let processed =
                    process_channel(&mono, noise_duration_sec, strength, floor, sample_rate);
                for (f, &s) in processed.iter().enumerate() {
                    if f < num_frames {
                        result[f * channels + ch] = s;
                    }
                }
            }
            *samples = result;
            channels as u16
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

    #[test]
    fn output_same_length_as_input() {
        let sr = 44100u32;
        let n = sr as usize; // 1 second
        let input: Vec<f32> = (0..n)
            .map(|i| (2.0 * PI * 440.0 * i as f32 / sr as f32).sin())
            .collect();
        let output = process_channel(&input, 0.1, 0.85, 0.05, sr);
        assert_eq!(output.len(), input.len());
    }

    #[test]
    fn silent_input_stays_silent() {
        let sr = 44100u32;
        let n = sr as usize;
        let input = vec![0.0f32; n];
        let output = process_channel(&input, 0.1, 0.85, 0.05, sr);
        assert_eq!(output.len(), n);
        for (i, &s) in output.iter().enumerate() {
            assert!(
                s.abs() < 1e-5,
                "silent input produced non-zero output at sample {i}: {s}"
            );
        }
    }

    #[test]
    fn reduces_constant_noise_floor() {
        // Build: 0.5 s noise-only, then 0.5 s noise + loud tone.
        let sr = 44100u32;
        let half = sr as usize / 2;
        let noise_amp = 0.02f32;
        let tone_amp = 0.5f32;
        let tone_freq = 1000.0f32;

        let mut input = Vec::with_capacity(half * 2);
        // Noise-only region.
        for i in 0..half {
            // Simple pseudo-noise: sign-alternating scaled sine at high freq.
            let n = noise_amp * (2.0 * PI * 8000.0 * i as f32 / sr as f32).sin();
            input.push(n);
        }
        // Tone + noise region.
        for i in 0..half {
            let tone = tone_amp * (2.0 * PI * tone_freq * i as f32 / sr as f32).sin();
            let n = noise_amp * (2.0 * PI * 8000.0 * (i + half) as f32 / sr as f32).sin();
            input.push(tone + n);
        }

        let output = process_channel(&input, 0.5, 0.85, 0.05, sr);

        let noise_region_rms = rms(&output[..half]);
        let tone_region_rms = rms(&output[half..]);

        // The tone region should be substantially louder than the noise-only region.
        assert!(
            tone_region_rms > noise_region_rms * 5.0,
            "expected tone region RMS ({tone_region_rms:.4}) >> noise region RMS ({noise_region_rms:.4})"
        );
    }
}
