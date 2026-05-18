use serde::Deserialize;
use serde_json::{json, Value};
use session::{Clip, Track, TrackId};

use crate::schema::anthropic_tool;
use crate::tool::util::{append_state, load_head_state};
use crate::{Tool, ToolContext, ToolResult};

fn lcg_next(state: &mut u64) -> f32 {
    *state = state
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    (*state >> 33) as f32 / (u32::MAX as f32 / 2.0) - 1.0
}

#[allow(clippy::excessive_precision)]
pub(crate) fn generate_noise_samples(
    sr: u32,
    duration_sec: f32,
    amplitude: f32,
    noise_type: &str,
) -> Vec<f32> {
    let n = (sr as f32 * duration_sec) as usize;
    let mut rng: u64 = 0xdeadbeef_cafebabe;
    let white: Vec<f32> = (0..n).map(|_| lcg_next(&mut rng) * amplitude).collect();
    match noise_type {
        "pink" => {
            let mut b0 = 0.0f32;
            let mut b1 = 0.0f32;
            let mut b2 = 0.0f32;
            let mut b3 = 0.0f32;
            let mut b4 = 0.0f32;
            let mut b5 = 0.0f32;
            let mut b6 = 0.0f32;
            white
                .iter()
                .map(|&w| {
                    b0 = 0.99886 * b0 + w * 0.0555179;
                    b1 = 0.99332 * b1 + w * 0.0750759;
                    b2 = 0.96900 * b2 + w * 0.1538520;
                    b3 = 0.86650 * b3 + w * 0.3104856;
                    b4 = 0.55000 * b4 + w * 0.5329522;
                    b5 = -0.7616 * b5 - w * 0.0168980;
                    b6 = w * 0.115926;
                    (b0 + b1 + b2 + b3 + b4 + b5 + b6 + w * 0.5362) * 0.11
                })
                .collect()
        }
        "brown" => {
            let mut last = 0.0f32;
            white
                .iter()
                .map(|&w| {
                    last = (last + w * 0.02).clamp(-1.0, 1.0);
                    last
                })
                .collect()
        }
        _ => white,
    }
}

#[derive(Debug, Deserialize)]
struct Args {
    duration_sec: f32,
    amplitude: Option<f32>,
    noise_type: Option<String>,
}

pub struct GenerateNoiseTool;

impl Tool for GenerateNoiseTool {
    fn name(&self) -> &'static str {
        "generate_noise"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "generate_noise",
            "Generate a noise track (white, pink, or brown/Brownian noise) and add it as a new track.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "duration_sec": { "type": "number" },
                    "amplitude": { "type": "number", "default": 0.5 },
                    "noise_type": { "type": "string", "enum": ["white","pink","brown"], "default": "white" }
                },
                "required": ["duration_sec"]
            }),
        )
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };
        if args.duration_sec <= 0.0 {
            return Ok(ToolResult::Error("duration_sec must be positive".into()));
        }
        let amp = args.amplitude.unwrap_or(0.5).clamp(0.0, 1.0);
        let noise = args.noise_type.as_deref().unwrap_or("white").to_string();

        let mut state = match load_head_state(ctx) {
            Ok(s) => s,
            Err(e) => return Ok(ToolResult::Error(e)),
        };
        let sr = state.sample_rate;
        let samples = generate_noise_samples(sr, args.duration_sec, amp, &noise);

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
        let track_idx = state.tracks.len();
        state.tracks.push(Track {
            id: TrackId::new(),
            name: format!("{noise} noise"),
            clips: vec![Clip {
                source_path: path,
                start_in_track: 0,
                source_offset: 0,
                length: n_frames,
                content_hash: Some(hash_bytes),
                time_stretch_factor: None,
                pitch_shift_semitones: None,
                beat_grid: None,
                volume_envelope: vec![],
            }],
            gain_db: 0.0,
            pan: 0.0,
            muted: false,
            soloed: false,
            effects: vec![],
        });
        state.length_samples = state.length_samples.max(n_frames);
        let new_id = match append_state(
            ctx,
            state,
            format!("generate_noise {} {:.1}s", noise, args.duration_sec),
        ) {
            Ok(id) => id,
            Err(e) => return Ok(ToolResult::Error(e)),
        };
        Ok(ToolResult::Ok(json!({
            "node_id": new_id.to_hex(),
            "track_index": track_idx,
            "summary": format!(
                "Generated {} noise ({:.1}s) as track {}",
                noise, args.duration_sec, track_idx
            )
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::generate_noise_samples;

    #[test]
    fn noise_length_correct() {
        let samples = generate_noise_samples(44100, 1.0, 0.5, "white");
        assert_eq!(samples.len(), 44100);
    }

    #[test]
    fn pink_noise_length_correct() {
        let samples = generate_noise_samples(44100, 0.5, 0.5, "pink");
        assert_eq!(samples.len(), 22050);
    }
}
