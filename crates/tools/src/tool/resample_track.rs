use std::path::Path;

use blake3::Hasher;
use chrono::Utc;
use serde::Deserialize;
use serde_json::{json, Value};
use session::{NodeId, SessionNode};

use crate::schema::anthropic_tool;
use crate::tool::util::{check_track_index, load_head_state};
use crate::{Tool, ToolContext, ToolResult};

/// Linear interpolation resample: interleaved `channels`-channel input
/// at `src_rate` → output at `dst_rate`.
pub(crate) fn linear_resample(
    samples: &[f32],
    channels: usize,
    src_rate: u32,
    dst_rate: u32,
) -> Vec<f32> {
    if src_rate == dst_rate || samples.is_empty() {
        return samples.to_vec();
    }
    let channels = channels.max(1);
    let src_frames = samples.len() / channels;
    let dst_frames = (src_frames as f64 * dst_rate as f64 / src_rate as f64).round() as usize;
    let mut out = Vec::with_capacity(dst_frames * channels);
    for dst_frame in 0..dst_frames {
        let src_pos = dst_frame as f64 * src_rate as f64 / dst_rate as f64;
        let src_lo = src_pos.floor() as usize;
        let src_hi = (src_lo + 1).min(src_frames - 1);
        let frac = (src_pos - src_lo as f64) as f32;
        for ch in 0..channels {
            let lo = samples[src_lo * channels + ch];
            let hi = samples[src_hi * channels + ch];
            out.push(lo + frac * (hi - lo));
        }
    }
    out
}

#[derive(Debug, Deserialize)]
struct Args {
    track: usize,
    target_sample_rate: u32,
}

pub struct ResampleTrackTool;

impl Tool for ResampleTrackTool {
    fn name(&self) -> &'static str {
        "resample_track"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "resample_track",
            "Resample a track to a different sample rate using linear interpolation. Common rates: 22050, 44100, 48000, 96000. Appends a new session node.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "track": { "type": "integer", "description": "Track index" },
                    "target_sample_rate": { "type": "integer", "description": "Target sample rate in Hz (e.g. 44100, 48000)" }
                },
                "required": ["track", "target_sample_rate"]
            }),
        )
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };
        if args.target_sample_rate == 0 {
            return Ok(ToolResult::Error(
                "target_sample_rate must be positive".into(),
            ));
        }

        let mut state = match load_head_state(ctx) {
            Ok(s) => s,
            Err(e) => return Ok(ToolResult::Error(e)),
        };
        if let Err(e) = check_track_index(&state.tracks, args.track) {
            return Ok(ToolResult::Error(e));
        }
        let clip = match state.tracks[args.track].clips.first().cloned() {
            Some(c) => c,
            None => {
                return Ok(ToolResult::Error(format!(
                    "track {} has no clips",
                    args.track
                )))
            }
        };

        let decoded = match audio_decoder::decode_file(&clip.source_path) {
            Ok(d) => d,
            Err(e) => {
                return Ok(ToolResult::Error(format!(
                    "failed to decode {}: {e}",
                    clip.source_path.display()
                )))
            }
        };
        let src_rate = decoded.sample_rate;
        let channels = decoded.channels as usize;
        if channels == 0 {
            return Ok(ToolResult::Error("source has zero channels".into()));
        }

        // Slice to clip window
        let stride = channels;
        let total_frames = decoded.samples.len() / stride;
        let src_start = clip.source_offset.min(total_frames as u64) as usize;
        let src_end = clip
            .source_offset
            .saturating_add(clip.length)
            .min(total_frames as u64) as usize;
        let window = &decoded.samples[src_start * stride..src_end * stride];

        let resampled = linear_resample(window, channels, src_rate, args.target_sample_rate);

        // CAS write
        let parent = clip.source_path.parent().unwrap_or_else(|| Path::new("."));
        let derived_dir = parent.join("derived");
        if let Err(e) = std::fs::create_dir_all(&derived_dir) {
            return Ok(ToolResult::Error(format!(
                "failed to create derived dir: {e}"
            )));
        }
        let mut hasher = Hasher::new();
        for s in &resampled {
            hasher.update(&s.to_le_bytes());
        }
        let hash = hasher.finalize();
        let hash_hex = hash.to_hex().to_string();
        let cas_path = derived_dir.join(format!("{hash_hex}.wav"));

        if !cas_path.exists() {
            if let Err(e) = audio_engine::write_wav(
                &resampled,
                args.target_sample_rate,
                channels as u16,
                &cas_path,
            ) {
                return Ok(ToolResult::Error(format!("failed to write WAV: {e}")));
            }
        }

        let new_length_frames = (resampled.len() / channels) as u64;
        let clip_mut = &mut state.tracks[args.track].clips[0];
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

        let label = format!(
            "resample_track {} {}Hz→{}Hz",
            args.track, src_rate, args.target_sample_rate
        );
        let node = SessionNode {
            id: NodeId([0u8; 32]),
            parent: None,
            created_at: Utc::now(),
            label: Some(label.clone()),
            reasoning: None,
            state,
        };
        let new_id = match ctx.store.append(node) {
            Ok(id) => id,
            Err(e) => return Ok(ToolResult::Error(format!("session append failed: {e}"))),
        };

        Ok(ToolResult::Ok(json!({
            "node_id": new_id.to_hex(),
            "summary": label,
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::linear_resample;

    #[test]
    fn resample_doubles_length() {
        let input: Vec<f32> = (0..100).map(|i| i as f32 / 100.0).collect();
        let output = linear_resample(&input, 1, 22050, 44100);
        assert!(
            output.len() >= 195 && output.len() <= 205,
            "expected ~200 samples, got {}",
            output.len()
        );
    }

    #[test]
    fn resample_halves_length() {
        let input: Vec<f32> = (0..200).map(|i| i as f32 / 200.0).collect();
        let output = linear_resample(&input, 1, 44100, 22050);
        assert!(
            output.len() >= 95 && output.len() <= 105,
            "expected ~100 samples, got {}",
            output.len()
        );
    }

    #[test]
    fn resample_same_rate_unchanged() {
        let input = vec![0.1f32, 0.5, -0.3, 0.8];
        let output = linear_resample(&input, 1, 44100, 44100);
        assert_eq!(output.len(), input.len());
        for (a, b) in input.iter().zip(output.iter()) {
            assert!((a - b).abs() < 1e-5);
        }
    }
}
