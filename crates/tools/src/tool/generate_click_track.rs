//! Generate a tempo-aligned percussion click as a new mono track.

use serde::Deserialize;
use serde_json::{json, Value};

use crate::schema::anthropic_tool;
use crate::tool::generated_audio::{append_generated, frame_count, MAX_DURATION_SEC};
use crate::tool::util::{check_track_index, lane_audio_path, load_head_state_or_empty};
use crate::{Tool, ToolContext, ToolResult};

/// Above this rate a click is no longer musically useful, and a 600-second
/// request could require millions of pulse starts. The 10 ms pulse also fits
/// within every beat at this limit.
const MAX_BPM: f64 = 6_000.0;

#[derive(Debug, Deserialize)]
struct Args {
    bpm: Option<f64>,
    track: Option<usize>,
    bars: Option<u64>,
    duration_sec: Option<f64>,
    beats_per_bar: Option<u32>,
    offset_sec: Option<f64>,
    amplitude: Option<f64>,
}

pub struct GenerateClickTrackTool;

impl Tool for GenerateClickTrackTool {
    fn name(&self) -> &'static str {
        "generate_click_track"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "generate_click_track",
            "Generate a percussive click track. Supply a BPM or a track to detect tempo from. Give exactly one of bars or duration_sec; duration_sec includes offset_sec. Downbeats are accented every beats_per_bar beats. Returns the new track index. BPM must be positive and at most 6000; clip length is at most 600 seconds.",
            json!({
                "type": "object",
                "properties": {
                    "bpm": { "type": "number", "description": "Explicit tempo in beats per minute; takes precedence over track." },
                    "track": { "type": "integer", "description": "Track index to measure tempo from when bpm is absent." },
                    "bars": { "type": "integer", "description": "Number of bars to generate (positive)." },
                    "duration_sec": { "type": "number", "description": "Total clip duration including the offset (seconds, >0 and <=600)." },
                    "beats_per_bar": { "type": "integer", "default": 4, "description": "Beats in each bar, 1..32." },
                    "offset_sec": { "type": "number", "default": 0, "description": "Initial silence before the first click, in seconds." },
                    "amplitude": { "type": "number", "default": 0.5, "description": "Downbeat peak amplitude, 0..1." }
                },
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
        let beats_per_bar = args.beats_per_bar.unwrap_or(4);
        if !(1..=32).contains(&beats_per_bar) {
            return Ok(ToolResult::Error("beats_per_bar must be 1..32".into()));
        }
        let offset = args.offset_sec.unwrap_or(0.0);
        if !offset.is_finite() || offset < 0.0 {
            return Ok(ToolResult::Error(
                "offset_sec must be finite and nonnegative".into(),
            ));
        }
        let amplitude = args.amplitude.unwrap_or(0.5);
        if !amplitude.is_finite() || !(0.0..=1.0).contains(&amplitude) {
            return Ok(ToolResult::Error(
                "amplitude must be between 0 and 1".into(),
            ));
        }
        if (args.bars.is_some() && args.duration_sec.is_some())
            || (args.bars.is_none() && args.duration_sec.is_none())
        {
            return Ok(ToolResult::Error(
                "give exactly one of bars or duration_sec".into(),
            ));
        }
        if args.bars == Some(0) {
            return Ok(ToolResult::Error("bars must be positive".into()));
        }
        let state = match load_head_state_or_empty(ctx) {
            Ok(state) => state,
            Err(error) => return Ok(ToolResult::Error(error)),
        };
        let bpm = match args.bpm {
            Some(bpm) => bpm,
            None => {
                let Some(track) = args.track else {
                    return Ok(ToolResult::Error(
                        "give bpm or track to derive tempo".into(),
                    ));
                };
                if let Err(error) = check_track_index(&state.tracks, track) {
                    return Ok(ToolResult::Error(error));
                }
                let Some(path) =
                    lane_audio_path(ctx.store.project_dir(), &state.tracks[track].clips)
                else {
                    return Ok(ToolResult::Error(format!("track {track} has no audio")));
                };
                let decoded = match audio_decoder::decode_file(&path) {
                    Ok(decoded) => decoded,
                    Err(error) => return Ok(ToolResult::Error(format!("decode failed: {error}"))),
                };
                let report = match audio_analysis::analyze(&decoded) {
                    Ok(report) => report,
                    Err(error) => {
                        return Ok(ToolResult::Error(format!("analysis failed: {error}")))
                    }
                };
                let Some(bpm) = report.bpm else {
                    return Ok(ToolResult::Error(format!(
                        "track {track} has no steady beat"
                    )));
                };
                bpm as f64
            }
        };
        if !bpm.is_finite() || bpm <= 0.0 || bpm > MAX_BPM {
            return Ok(ToolResult::Error(format!(
                "bpm must be positive and at most {MAX_BPM}"
            )));
        }
        let period = 60.0 / bpm;
        let (duration, beats) = if let Some(bars) = args.bars {
            let Some(beats) = bars.checked_mul(u64::from(beats_per_bar)) else {
                return Ok(ToolResult::Error("bar count is too large".into()));
            };
            (offset + beats as f64 * period, Some(beats))
        } else {
            let duration = args
                .duration_sec
                .expect("validated exactly one length mode");
            if !duration.is_finite() || duration <= 0.0 || duration > MAX_DURATION_SEC {
                return Ok(ToolResult::Error(format!(
                    "duration_sec must be positive and at most {MAX_DURATION_SEC}"
                )));
            }
            if offset >= duration {
                return Ok(ToolResult::Error(
                    "offset_sec must be less than duration_sec".into(),
                ));
            }
            (duration, None)
        };
        let frames = match frame_count(state.sample_rate, duration) {
            Ok(frames) => frames,
            Err(error) => return Ok(ToolResult::Error(error)),
        };
        let rate = state.sample_rate as f64;
        let pulse_frames = ((state.sample_rate as f64 * 0.010).round() as usize)
            .max(1)
            .min((rate * period).floor() as usize);
        if pulse_frames == 0 {
            return Ok(ToolResult::Error(
                "bpm is too high for this sample rate".into(),
            ));
        }
        let mut samples = vec![0.0f32; frames];
        let mut beat = 0u64;
        loop {
            if beats.is_some_and(|count| beat >= count) {
                break;
            }
            let time = offset + beat as f64 * period;
            if time >= duration {
                break;
            }
            let start = (time * rate).round() as usize;
            if start >= frames {
                break;
            }
            let peak = if beat.is_multiple_of(u64::from(beats_per_bar)) {
                amplitude
            } else {
                amplitude * 0.6
            };
            for i in 0..pulse_frames.min(frames - start) {
                let envelope = (1.0 - i as f64 / pulse_frames as f64).powi(2);
                let click = (std::f64::consts::TAU * 1_200.0 * i as f64 / rate).cos();
                samples[start + i] = (peak * envelope * click) as f32;
            }
            beat += 1;
        }
        let result = append_generated(
            ctx,
            state,
            samples,
            format!("{bpm:.1} BPM click track"),
            format!("generate_click_track {bpm:.1} BPM {duration:.3}s"),
        )?;
        Ok(result)
    }
}
