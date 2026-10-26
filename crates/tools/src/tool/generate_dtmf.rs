use serde::Deserialize;
use serde_json::{json, Value};

use crate::schema::anthropic_tool;
use crate::tool::generated_audio::{append_generated, frame_count, MAX_DURATION_SEC};
use crate::tool::util::load_head_state_or_empty;
use crate::{Tool, ToolContext, ToolResult};

const DEFAULT_TONE_DURATION_SEC: f64 = 0.1;
const DEFAULT_GAP_DURATION_SEC: f64 = 0.05;
const DEFAULT_AMPLITUDE: f64 = 0.5;
const MIN_TONE_DURATION_SEC: f64 = 0.04;
const RAMP_DURATION_SEC: f64 = 0.005;

#[derive(Debug, Deserialize)]
struct Args {
    digits: String,
    tone_duration_sec: Option<f64>,
    gap_duration_sec: Option<f64>,
    amplitude: Option<f64>,
}

fn frequencies(digit: char) -> Option<(f64, f64)> {
    let row = match digit {
        '1' | '2' | '3' | 'A' => 697.0,
        '4' | '5' | '6' | 'B' => 770.0,
        '7' | '8' | '9' | 'C' => 852.0,
        '0' | '*' | '#' | 'D' => 941.0,
        _ => return None,
    };
    let column = match digit {
        '1' | '4' | '7' | '*' => 1209.0,
        '2' | '5' | '8' | '0' => 1336.0,
        '3' | '6' | '9' | '#' => 1477.0,
        'A' | 'B' | 'C' | 'D' => 1633.0,
        _ => return None,
    };
    Some((row, column))
}

fn synthesize_digit(
    output: &mut Vec<f32>,
    sample_rate: u32,
    frames: usize,
    amplitude: f64,
    row_hz: f64,
    column_hz: f64,
    ramp_frames: usize,
) {
    let rate = sample_rate as f64;
    for frame in 0..frames {
        let fade_in = if ramp_frames == 0 {
            1.0
        } else {
            (frame as f64 / ramp_frames as f64).min(1.0)
        };
        let remaining = frames - 1 - frame;
        let fade_out = if ramp_frames == 0 {
            1.0
        } else {
            (remaining as f64 / ramp_frames as f64).min(1.0)
        };
        let envelope = fade_in.min(fade_out);
        // Phase in cycles, reduced before the sine, so the result depends
        // less on the platform's libm at large arguments.
        let row = (std::f64::consts::TAU * (row_hz * frame as f64 / rate).fract()).sin();
        let column = (std::f64::consts::TAU * (column_hz * frame as f64 / rate).fract()).sin();
        output.push(((row + column) * (amplitude * 0.5) * envelope) as f32);
    }
}

pub struct GenerateDtmfTool;

impl Tool for GenerateDtmfTool {
    fn name(&self) -> &'static str {
        "generate_dtmf"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "generate_dtmf",
            "Generate standard dual-tone multifrequency keypad audio and add it as a new track.",
            json!({
                "type": "object",
                "properties": {
                    "digits": {
                        "type": "string",
                        "description": "One or more keypad symbols: 0-9, *, #, or A-D"
                    },
                    "tone_duration_sec": {
                        "type": "number",
                        "minimum": MIN_TONE_DURATION_SEC,
                        "default": DEFAULT_TONE_DURATION_SEC,
                        "description": "Duration of each tone in seconds (minimum 40 ms; total up to 600 s, less above 96 kHz)"
                    },
                    "gap_duration_sec": {
                        "type": "number",
                        "minimum": 0,
                        "default": DEFAULT_GAP_DURATION_SEC,
                        "description": "Silence between tones, in seconds (0 for none)"
                    },
                    "amplitude": {
                        "type": "number",
                        "minimum": 0,
                        "maximum": 1,
                        "default": DEFAULT_AMPLITUDE,
                        "description": "Peak combined tone amplitude from 0 to 1"
                    }
                },
                "required": ["digits"],
                "additionalProperties": false
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
        if args.digits.is_empty() {
            return Ok(ToolResult::Error("digits must not be empty".into()));
        }

        if args
            .digits
            .chars()
            .map(|digit| digit.to_ascii_uppercase())
            .any(|digit| frequencies(digit).is_none())
        {
            return Ok(ToolResult::Error(
                "digits may contain only 0-9, *, #, or A-D".into(),
            ));
        }
        let digit_count = args.digits.chars().count();

        let tone_duration = args.tone_duration_sec.unwrap_or(DEFAULT_TONE_DURATION_SEC);
        let gap_duration = args.gap_duration_sec.unwrap_or(DEFAULT_GAP_DURATION_SEC);
        let amplitude = args.amplitude.unwrap_or(DEFAULT_AMPLITUDE);
        if !tone_duration.is_finite() || tone_duration < MIN_TONE_DURATION_SEC {
            return Ok(ToolResult::Error(
                "tone_duration_sec must be finite and at least 0.04 seconds".into(),
            ));
        }
        if !gap_duration.is_finite() || gap_duration < 0.0 {
            return Ok(ToolResult::Error(
                "gap_duration_sec must be finite and non-negative".into(),
            ));
        }
        if !amplitude.is_finite() || !(0.0..=1.0).contains(&amplitude) {
            return Ok(ToolResult::Error(
                "amplitude must be between 0 and 1".into(),
            ));
        }

        let state = match load_head_state_or_empty(ctx) {
            Ok(state) => state,
            Err(error) => return Ok(ToolResult::Error(error)),
        };
        let sample_rate = state.sample_rate;
        let tone_frames = match frame_count(sample_rate, tone_duration) {
            Ok(frames) => frames,
            Err(error) => return Ok(ToolResult::Error(error)),
        };
        // A gap that rounds to no frames is no gap; the total below is
        // bounded by the 600 s check.
        let gap_frames = (f64::from(sample_rate) * gap_duration).round() as usize;
        if [697.0, 770.0, 852.0, 941.0, 1209.0, 1336.0, 1477.0, 1633.0]
            .iter()
            .any(|&frequency| frequency >= sample_rate as f64 / 2.0)
        {
            return Ok(ToolResult::Error(
                "session sample rate is too low for DTMF frequencies".into(),
            ));
        }
        let total_duration = tone_duration * digit_count as f64
            + gap_duration * digit_count.saturating_sub(1) as f64;
        if !total_duration.is_finite() || total_duration > MAX_DURATION_SEC {
            return Ok(ToolResult::Error(format!(
                "generated audio must not exceed {MAX_DURATION_SEC} seconds"
            )));
        }
        let total_frames = match tone_frames
            .checked_mul(digit_count)
            .and_then(|tones| gap_frames.checked_mul(digit_count - 1)?.checked_add(tones))
        {
            Some(frames) => {
                if let Err(error) = frame_count(sample_rate, frames as f64 / sample_rate as f64) {
                    return Ok(ToolResult::Error(error));
                }
                frames
            }
            None => {
                return Ok(ToolResult::Error(format!(
                    "generated audio must not exceed {MAX_DURATION_SEC} seconds"
                )));
            }
        };
        let digits: Vec<char> = args
            .digits
            .chars()
            .map(|digit| digit.to_ascii_uppercase())
            .collect();

        let ramp_frames = frame_count(sample_rate, RAMP_DURATION_SEC)
            .unwrap_or(1)
            .min(tone_frames / 2);
        let mut samples = Vec::with_capacity(total_frames);
        for (index, digit) in digits.iter().copied().enumerate() {
            let (row_hz, column_hz) = frequencies(digit).expect("digits validated above");
            synthesize_digit(
                &mut samples,
                sample_rate,
                tone_frames,
                amplitude,
                row_hz,
                column_hz,
                ramp_frames,
            );
            if index + 1 < digits.len() {
                samples.resize(samples.len() + gap_frames, 0.0);
            }
        }

        let shown: String = digits.iter().take(32).collect();
        let more = if digits.len() > 32 { "…" } else { "" };
        let name = format!("DTMF {shown}{more}");
        let action = format!("generate_dtmf {} digits", digits.len());
        append_generated(ctx, state, samples, name, action)
    }
}
