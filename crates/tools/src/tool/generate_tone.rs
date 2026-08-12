use std::f32::consts::PI;

use serde::Deserialize;
use serde_json::{json, Value};
use session::{Clip, Track, TrackId};

use crate::schema::anthropic_tool;
use crate::tool::util::{append_state, load_head_state};
use crate::{Tool, ToolContext, ToolResult};

pub(crate) fn synthesize_tone(
    sr: u32,
    duration_sec: f32,
    freq_hz: f32,
    amplitude: f32,
    waveform: &str,
) -> Vec<f32> {
    let n = (sr as f32 * duration_sec) as usize;
    (0..n)
        .map(|i| {
            let t = i as f32 / sr as f32;
            let phase = 2.0 * PI * freq_hz * t;
            let raw = match waveform {
                "square" => {
                    if phase.sin() >= 0.0 {
                        1.0f32
                    } else {
                        -1.0
                    }
                }
                "sawtooth" => 2.0 * (freq_hz * t - (freq_hz * t + 0.5).floor()),
                "triangle" => 1.0 - 4.0 * (freq_hz * t - (freq_hz * t + 0.25).floor()).abs(),
                _ => phase.sin(),
            };
            raw * amplitude
        })
        .collect()
}

#[derive(Debug, Deserialize)]
struct Args {
    frequency_hz: f32,
    duration_sec: f32,
    amplitude: Option<f32>,
    waveform: Option<String>,
}

pub struct GenerateToneTool;

impl Tool for GenerateToneTool {
    fn name(&self) -> &'static str {
        "generate_tone"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "generate_tone",
            "Synthesize a tone (sine, square, sawtooth, or triangle wave) and add it as a new track. Returns the new track index.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "frequency_hz": { "type": "number", "description": "Tone frequency in Hz" },
                    "duration_sec": { "type": "number", "description": "Duration in seconds" },
                    "amplitude": { "type": "number", "default": 0.5, "description": "Peak amplitude 0..1" },
                    "waveform": { "type": "string", "enum": ["sine","square","sawtooth","triangle"], "default": "sine" }
                },
                "required": ["frequency_hz", "duration_sec"]
            }),
        )
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };
        if args.frequency_hz <= 0.0 {
            return Ok(ToolResult::Error("frequency_hz must be positive".into()));
        }
        if args.duration_sec <= 0.0 {
            return Ok(ToolResult::Error("duration_sec must be positive".into()));
        }
        let amp = args.amplitude.unwrap_or(0.5).clamp(0.0, 1.0);
        let wave = args.waveform.as_deref().unwrap_or("sine").to_string();

        let mut state = match load_head_state(ctx) {
            Ok(s) => s,
            Err(e) => return Ok(ToolResult::Error(e)),
        };
        let sr = state.sample_rate;
        let samples = synthesize_tone(sr, args.duration_sec, args.frequency_hz, amp, &wave);

        let gen_dir = ctx.store.project_dir().join("generated");
        if let Err(e) = std::fs::create_dir_all(&gen_dir) {
            return Ok(ToolResult::Error(format!("mkdir failed: {e}")));
        }
        let mut hasher = blake3::Hasher::new();
        for s in &samples {
            hasher.update(&s.to_le_bytes());
        }
        let hash = hasher.finalize();
        let hash_bytes: [u8; 32] = *hash.as_bytes();
        let path = gen_dir.join(format!("{}.wav", hash.to_hex()));
        if !path.exists() {
            if let Err(e) = audio_engine::write_wav(&samples, sr, 1, &path) {
                return Ok(ToolResult::Error(format!("write_wav failed: {e}")));
            }
        }
        let n_frames = samples.len() as u64;
        let clip = Clip {
            source_path: path,
            start_in_track: 0,
            source_offset: 0,
            length: n_frames,
            content_hash: Some(hash_bytes),
            time_stretch_factor: None,
            pitch_shift_semitones: None,
            beat_grid: None,
            volume_envelope: vec![],
        };
        let track_idx = state.tracks.len();
        state.tracks.push(Track {
            id: TrackId::new(),
            name: format!("{:.0}Hz {} tone", args.frequency_hz, wave),
            clips: vec![clip],
            gain_db: 0.0,
            pan: 0.0,
            muted: false,
            soloed: false,
            effects: vec![],
            sends: Vec::new(),
        });
        state.length_samples = state.length_samples.max(n_frames);
        let new_id = match append_state(
            ctx,
            state,
            format!(
                "generate_tone {:.0}Hz {}s",
                args.frequency_hz, args.duration_sec
            ),
        ) {
            Ok(id) => id,
            Err(e) => return Ok(ToolResult::Error(e)),
        };
        Ok(ToolResult::Ok(json!({
            "node_id": new_id.to_hex(),
            "track_index": track_idx,
            "summary": format!(
                "Generated {:.0}Hz {} tone ({:.1}s) as track {}",
                args.frequency_hz, wave, args.duration_sec, track_idx
            )
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::synthesize_tone;

    #[test]
    fn sine_length_correct() {
        let samples = synthesize_tone(44100, 1.0, 440.0, 0.5, "sine");
        assert_eq!(samples.len(), 44100);
    }

    #[test]
    fn sine_peak_near_amplitude() {
        let samples = synthesize_tone(44100, 0.1, 440.0, 0.5, "sine");
        let peak = samples.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        assert!(
            (peak - 0.5).abs() < 0.01,
            "peak should be near 0.5, got {peak}"
        );
    }
}
