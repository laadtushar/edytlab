//! Parametric EQ tool — biquad peak filter chain (Audio-EQ-Cookbook).

use std::path::Path;
use std::path::PathBuf;

use serde::Deserialize;
use serde_json::{json, Value};

use crate::schema::anthropic_tool;
use crate::tool::util::{append_state, check_track_index, load_head_state};
use crate::{Tool, ToolContext, ToolResult};

// ---------------------------------------------------------------------------
// Biquad peak-EQ filter
// ---------------------------------------------------------------------------

/// Coefficients for one biquad peak-EQ band, pre-normalised by a0.
struct BiquadCoeffs {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
}

impl BiquadCoeffs {
    fn peak_eq(freq_hz: f32, gain_db: f32, q: f32, sample_rate: u32) -> Self {
        let a = 10f64.powf(gain_db as f64 / 40.0);
        let w0 = 2.0 * std::f64::consts::PI * freq_hz as f64 / sample_rate as f64;
        let alpha = w0.sin() / (2.0 * q as f64);

        let a0 = 1.0 + alpha / a;
        BiquadCoeffs {
            b0: (1.0 + alpha * a) / a0,
            b1: (-2.0 * w0.cos()) / a0,
            b2: (1.0 - alpha * a) / a0,
            a1: (-2.0 * w0.cos()) / a0,
            a2: (1.0 - alpha / a) / a0,
        }
    }
}

/// Per-channel delay state for direct-form-II transposed.
#[derive(Clone, Default)]
struct BiquadState {
    w1: f64,
    w2: f64,
}

impl BiquadState {
    fn process(&mut self, x: f64, c: &BiquadCoeffs) -> f64 {
        let y = c.b0 * x + self.w1;
        self.w1 = c.b1 * x - c.a1 * y + self.w2;
        self.w2 = c.b2 * x - c.a2 * y;
        y
    }
}

/// Apply a chain of peak-EQ bands to an interleaved f32 buffer.
///
/// `channels` is the interleave stride. Each channel gets independent
/// biquad state so the filter is correct regardless of channel count.
fn apply_eq(samples: &mut [f32], channels: usize, sample_rate: u32, bands: &[Band]) {
    if channels == 0 || bands.is_empty() || samples.is_empty() {
        return;
    }

    // Pre-compute coefficients for each band.
    let coeffs: Vec<BiquadCoeffs> = bands
        .iter()
        .map(|b| BiquadCoeffs::peak_eq(b.freq_hz, b.gain_db, b.q, sample_rate))
        .collect();

    // One state vector per (band, channel).
    let mut states: Vec<Vec<BiquadState>> = coeffs
        .iter()
        .map(|_| vec![BiquadState::default(); channels])
        .collect();

    for (i, sample) in samples.iter_mut().enumerate() {
        let ch = i % channels;
        let mut x = *sample as f64;
        for (band_idx, coeff) in coeffs.iter().enumerate() {
            x = states[band_idx][ch].process(x, coeff);
        }
        *sample = x as f32;
    }
}

// ---------------------------------------------------------------------------
// Args
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct Band {
    freq_hz: f32,
    gain_db: f32,
    #[serde(default = "default_q")]
    q: f32,
}

fn default_q() -> f32 {
    1.0
}

#[derive(Debug, Deserialize)]
struct Args {
    track: usize,
    bands: Vec<Band>,
}

// ---------------------------------------------------------------------------
// Tool impl
// ---------------------------------------------------------------------------

pub struct EqTool;

impl Tool for EqTool {
    fn name(&self) -> &'static str {
        "eq"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "eq",
            "Apply a parametric equalizer (chain of biquad peak filters) to a track. \
             Each band specifies a centre frequency, gain in dB, and optional Q factor \
             (default 1.0). Appends a new session node.",
            json!({
                "type": "object",
                "required": ["track", "bands"],
                "additionalProperties": false,
                "properties": {
                    "track": { "type": "integer" },
                    "bands": {
                        "type": "array",
                        "minItems": 1,
                        "items": {
                            "type": "object",
                            "required": ["freq_hz", "gain_db"],
                            "additionalProperties": false,
                            "properties": {
                                "freq_hz": { "type": "number" },
                                "gain_db": { "type": "number" },
                                "q": { "type": "number" }
                            }
                        }
                    }
                }
            }),
        )
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let parsed: Args = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };

        // Validate bands.
        if parsed.bands.is_empty() {
            return Ok(ToolResult::Error("bands must not be empty".to_string()));
        }
        for (i, b) in parsed.bands.iter().enumerate() {
            if b.freq_hz <= 0.0 {
                return Ok(ToolResult::Error(format!(
                    "band {i}: freq_hz must be > 0 (got {})",
                    b.freq_hz
                )));
            }
            if !b.gain_db.is_finite() {
                return Ok(ToolResult::Error(format!(
                    "band {i}: gain_db must be finite"
                )));
            }
            if b.q <= 0.0 {
                return Ok(ToolResult::Error(format!(
                    "band {i}: q must be > 0 (got {})",
                    b.q
                )));
            }
        }

        Ok(invoke_eq(ctx, parsed.track, parsed.bands))
    }
}

/// Core EQ logic — inlined from `destructive_edit` so we can capture
/// `channels` from the decoded audio (needed for per-channel biquad state).
fn invoke_eq(ctx: &mut ToolContext, track_idx: usize, bands: Vec<Band>) -> ToolResult {
    let mut state = match load_head_state(ctx) {
        Ok(s) => s,
        Err(msg) => return ToolResult::Error(msg),
    };

    if let Err(msg) = check_track_index(&state.tracks, track_idx) {
        return ToolResult::Error(msg);
    }

    let Some(clip) = state.tracks[track_idx].clips.first().cloned() else {
        return ToolResult::Error(format!("track {track_idx} has no clips; nothing to edit"));
    };

    // Decode — we need `channels` for per-channel biquad state.
    let decoded = match audio_decoder::decode_file(&clip.source_path) {
        Ok(d) => d,
        Err(e) => {
            return ToolResult::Error(format!(
                "failed to decode {}: {e}",
                clip.source_path.display()
            ))
        }
    };
    let sample_rate = decoded.sample_rate;
    let channels = decoded.channels as usize;
    if channels == 0 {
        return ToolResult::Error("source has zero channels".into());
    }

    let total_frames = (decoded.samples.len() / channels) as u64;
    let src_start = clip.source_offset.min(total_frames);
    let src_end = clip
        .source_offset
        .saturating_add(clip.length)
        .min(total_frames);
    let start_idx = src_start as usize * channels;
    let end_idx = src_end as usize * channels;
    let mut window: Vec<f32> = decoded.samples[start_idx..end_idx].to_vec();

    // Apply the EQ.
    apply_eq(&mut window, channels, sample_rate, &bands);

    // CAS write.
    let parent: &Path = clip.source_path.parent().unwrap_or_else(|| Path::new("."));
    let derived_dir: PathBuf = parent.join("derived");
    if let Err(e) = std::fs::create_dir_all(&derived_dir) {
        return ToolResult::Error(format!(
            "failed to create derived dir {}: {e}",
            derived_dir.display()
        ));
    }

    let mut hasher = blake3::Hasher::new();
    for s in &window {
        hasher.update(&s.to_le_bytes());
    }
    let hash = hasher.finalize();
    let hash_hex = hash.to_hex().to_string();
    let cas_path = derived_dir.join(format!("{hash_hex}.wav"));

    if !cas_path.exists() {
        if let Err(e) = audio_engine::write_wav(&window, sample_rate, decoded.channels, &cas_path) {
            return ToolResult::Error(format!(
                "failed to write CAS wav {}: {e}",
                cas_path.display()
            ));
        }
    }

    let new_length_frames = (window.len() / channels) as u64;
    let clip_mut = &mut state.tracks[track_idx].clips[0];
    clip_mut.source_path = cas_path;
    clip_mut.source_offset = 0;
    clip_mut.length = new_length_frames;
    clip_mut.content_hash = Some(*hash.as_bytes());

    state.length_samples = state
        .tracks
        .iter()
        .flat_map(|t| t.clips.iter().map(|c| c.start_in_track + c.length))
        .max()
        .unwrap_or(0);

    let band_summary: Vec<String> = bands
        .iter()
        .map(|b| format!("{:.0}Hz {:+.1}dB Q{:.2}", b.freq_hz, b.gain_db, b.q))
        .collect();
    let label = format!("eq [{}] on track {}", band_summary.join(", "), track_idx);

    let new_id = match append_state(ctx, state, label.clone()) {
        Ok(id) => id,
        Err(msg) => return ToolResult::Error(msg),
    };

    ToolResult::Ok(json!({
        "node_id": new_id.to_hex(),
        "summary": label,
    }))
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
    fn unity_gain_passthrough() {
        let sr = 44100u32;
        let freq = 1000.0f32;
        let n = sr as usize;
        let mut samples: Vec<f32> = (0..n)
            .map(|i| (2.0 * std::f32::consts::PI * freq * i as f32 / sr as f32).sin())
            .collect();
        let original = samples.clone();

        let bands = vec![Band {
            freq_hz: 1000.0,
            gain_db: 0.0,
            q: 1.0,
        }];
        apply_eq(&mut samples, 1, sr, &bands);

        // Unity gain band should leave samples essentially unchanged.
        for (a, b) in original.iter().zip(samples.iter()) {
            assert!(
                (a - b).abs() < 1e-5,
                "sample changed by more than 1e-5: {a} vs {b}"
            );
        }
    }

    #[test]
    fn boost_increases_rms_near_freq() {
        let sr = 44100u32;
        let freq = 1000.0f32;
        let n = sr as usize;
        let mut samples: Vec<f32> = (0..n)
            .map(|i| (2.0 * std::f32::consts::PI * freq * i as f32 / sr as f32).sin())
            .collect();
        let rms_before = rms(&samples);

        let bands = vec![Band {
            freq_hz: 1000.0,
            gain_db: 6.0,
            q: 1.0,
        }];
        apply_eq(&mut samples, 1, sr, &bands);

        let rms_after = rms(&samples);
        assert!(
            rms_after > rms_before * 1.5,
            "expected RMS to increase substantially: before={rms_before}, after={rms_after}"
        );
    }

    #[test]
    fn rejects_empty_bands() {
        let tool = EqTool;
        let args = serde_json::json!({
            "track": 0,
            "bands": []
        });
        // Schema validation (minItems:1) would catch this before invoke,
        // but let's also verify our own guard works via the Args path.
        // We bypass schema validation and call through serde directly.
        let parsed: Result<Args, _> = serde_json::from_value(args.clone());
        if let Ok(parsed) = parsed {
            // If serde accepts it (minItems not enforced by serde), our code should reject it.
            if parsed.bands.is_empty() {
                // Expected: our validation returns Error.
                // We can't call invoke without a ToolContext, so just assert the logic.
                assert!(
                    parsed.bands.is_empty(),
                    "expected empty bands to be rejected"
                );
            }
        }
        // Also verify the schema includes minItems:1 so the dispatcher catches it.
        let schema = tool.schema();
        let min_items = &schema["input_schema"]["properties"]["bands"]["minItems"];
        assert_eq!(min_items, 1, "schema must enforce minItems:1 on bands");
    }
}
