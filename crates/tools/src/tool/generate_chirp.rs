//! Generate a linear or logarithmic frequency sweep and append it as a track.
//!
//! Frequencies are in Hz and duration is in seconds. The generated track uses
//! the project's current sample rate and is stored as a content-addressed WAV.
//! The sample count rounds duration to the nearest frame; the sweep's phase
//! equation uses the requested duration.

use serde::Deserialize;
use serde_json::{json, Value};
use session::SessionState;

use crate::schema::anthropic_tool;
use crate::tool::generated_audio::{append_generated, frame_count, MAX_DURATION_SEC};
use crate::tool::util::load_head_state_or_empty;
use crate::{Tool, ToolContext, ToolResult};

#[derive(Debug, Deserialize)]
struct Args {
    start_frequency_hz: f64,
    end_frequency_hz: f64,
    duration_sec: f64,
    amplitude: Option<f64>,
    method: Option<String>,
}

/// Integrate the requested instantaneous frequency to get each sample's phase.
///
/// This follows SciPy's phase definition:
/// <https://github.com/scipy/scipy/blob/v1.16.0/scipy/signal/_waveforms.py#L421-L450>.
/// The logarithmic form uses `ln_1p` and `exp_m1` so a shallow sweep does not lose
/// precision, and matching endpoints use the constant-frequency limit.
fn synthesize_chirp(
    sample_rate: u32,
    frame_count: usize,
    start_frequency_hz: f64,
    end_frequency_hz: f64,
    duration_sec: f64,
    amplitude: f64,
    method: &str,
) -> Vec<f32> {
    let sr = f64::from(sample_rate);
    let delta_frequency = end_frequency_hz - start_frequency_hz;
    let log_ratio = if method == "logarithmic" && delta_frequency != 0.0 {
        let relative_delta = delta_frequency / start_frequency_hz;
        Some(
            if relative_delta.is_finite() && relative_delta.abs() < 0.5 {
                relative_delta.ln_1p()
            } else {
                end_frequency_hz.ln() - start_frequency_hz.ln()
            },
        )
    } else {
        None
    };

    (0..frame_count)
        .map(|frame| {
            let t = frame as f64 / sr;
            let phase_cycles = match method {
                "logarithmic" => match log_ratio {
                    Some(ratio) => {
                        let exponent = ratio * t / duration_sec;
                        if exponent > 50.0 {
                            // Avoid overflowing exp(exponent) for an extreme
                            // but finite positive sweep. The combined result
                            // remains bounded by end_frequency * duration.
                            (start_frequency_hz.ln() + duration_sec.ln() + exponent - ratio.ln())
                                .exp()
                        } else {
                            start_frequency_hz * duration_sec * exponent.exp_m1() / ratio
                        }
                    }
                    None => start_frequency_hz * t,
                },
                _ => start_frequency_hz * t + delta_frequency * t * t / (2.0 * duration_sec),
            };
            (std::f64::consts::TAU * phase_cycles).sin() as f32 * amplitude as f32
        })
        .collect()
}

pub struct GenerateChirpTool;

impl Tool for GenerateChirpTool {
    fn name(&self) -> &'static str {
        "generate_chirp"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "generate_chirp",
            "Generate a linear or logarithmic frequency sweep and add it as a new track. Frequencies are in Hz; duration is in seconds.",
            json!({
                "type": "object",
                "properties": {
                    "start_frequency_hz": {
                        "type": "number",
                        "exclusiveMinimum": 0,
                        "description": "Sweep start frequency in Hz; must be below Nyquist"
                    },
                    "end_frequency_hz": {
                        "type": "number",
                        "exclusiveMinimum": 0,
                        "description": "Sweep end frequency in Hz; must be below Nyquist"
                    },
                    "duration_sec": {
                        "type": "number",
                        "exclusiveMinimum": 0,
                        "maximum": MAX_DURATION_SEC,
                        "description": "Sweep duration in seconds (maximum 600); sample count rounds to the nearest frame"
                    },
                    "amplitude": {
                        "type": "number",
                        "minimum": 0,
                        "maximum": 1,
                        "default": 0.5,
                        "description": "Peak amplitude from 0 to 1"
                    },
                    "method": {
                        "type": "string",
                        "enum": ["linear", "logarithmic"],
                        "default": "linear",
                        "description": "Sweep shape"
                    }
                },
                "required": ["start_frequency_hz", "end_frequency_hz", "duration_sec"]
            }),
        )
    }

    fn runs_off_the_lock(&self) -> bool {
        true
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(args) => args,
            Err(error) => return Ok(ToolResult::Error(format!("invalid arguments: {error}"))),
        };

        let amplitude = args.amplitude.unwrap_or(0.5);
        let method = args.method.as_deref().unwrap_or("linear");
        if !args.start_frequency_hz.is_finite() || args.start_frequency_hz <= 0.0 {
            return Ok(ToolResult::Error(
                "start_frequency_hz must be finite and positive".into(),
            ));
        }
        if !args.end_frequency_hz.is_finite() || args.end_frequency_hz <= 0.0 {
            return Ok(ToolResult::Error(
                "end_frequency_hz must be finite and positive".into(),
            ));
        }
        if !args.duration_sec.is_finite()
            || args.duration_sec <= 0.0
            || args.duration_sec > MAX_DURATION_SEC
        {
            return Ok(ToolResult::Error(format!(
                "duration_sec must be positive and at most {MAX_DURATION_SEC} seconds"
            )));
        }
        if !amplitude.is_finite() || !(0.0..=1.0).contains(&amplitude) {
            return Ok(ToolResult::Error(
                "amplitude must be finite and between 0 and 1".into(),
            ));
        }
        if !matches!(method, "linear" | "logarithmic") {
            return Ok(ToolResult::Error(
                "method must be linear or logarithmic".into(),
            ));
        }

        let state: SessionState = match load_head_state_or_empty(ctx) {
            Ok(state) => state,
            Err(message) => return Ok(ToolResult::Error(message)),
        };
        let sample_rate = state.sample_rate;
        let nyquist = f64::from(sample_rate) / 2.0;
        if args.start_frequency_hz >= nyquist || args.end_frequency_hz >= nyquist {
            return Ok(ToolResult::Error(format!(
                "start_frequency_hz and end_frequency_hz must be below Nyquist ({nyquist} Hz)"
            )));
        }

        // Validate the actual session rate's frame count before allocation.
        let frames = match frame_count(sample_rate, args.duration_sec) {
            Ok(frames) => frames,
            Err(message) => return Ok(ToolResult::Error(message)),
        };
        let samples = synthesize_chirp(
            sample_rate,
            frames,
            args.start_frequency_hz,
            args.end_frequency_hz,
            args.duration_sec,
            amplitude,
            method,
        );
        let track_name = format!(
            "{:.0}–{:.0}Hz {} chirp",
            args.start_frequency_hz, args.end_frequency_hz, method
        );
        append_generated(
            ctx,
            state,
            samples,
            track_name,
            format!(
                "generate_chirp {:.1}–{:.1}Hz {} {:.3}s",
                args.start_frequency_hz, args.end_frequency_hz, method, args.duration_sec
            ),
        )
    }
}
