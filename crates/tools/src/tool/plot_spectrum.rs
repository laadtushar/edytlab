use rustfft::{num_complex::Complex, FftPlanner};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::schema::anthropic_tool;
use crate::tool::util::{check_track_index, load_head_state};
use crate::{Tool, ToolContext, ToolResult};

const FFT_SIZE: usize = 4096;

pub(crate) fn compute_fft_magnitude(samples: &[f32], _sr: u32, fft_size: usize) -> Vec<f32> {
    let mut planner = FftPlanner::new();
    let fft = planner.plan_fft_forward(fft_size);
    let mut buf: Vec<Complex<f32>> = (0..fft_size)
        .map(|i| {
            let window =
                0.5 * (1.0 - (2.0 * std::f32::consts::PI * i as f32 / (fft_size - 1) as f32).cos());
            let s = samples.get(i).copied().unwrap_or(0.0);
            Complex::new(s * window, 0.0)
        })
        .collect();
    fft.process(&mut buf);
    (0..fft_size / 2)
        .map(|i| {
            let mag = buf[i].norm() / fft_size as f32;
            if mag > 1e-10 {
                20.0 * mag.log10()
            } else {
                -120.0
            }
        })
        .collect()
}

#[derive(Debug, Deserialize)]
struct Args {
    track: usize,
    start_sec: f64,
    end_sec: f64,
}

pub struct PlotSpectrumTool;

impl Tool for PlotSpectrumTool {
    fn name(&self) -> &'static str {
        "plot_spectrum"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "plot_spectrum",
            "Compute the FFT magnitude spectrum of a track region. Returns frequency/magnitude data for display. Does not modify audio.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "track": { "type": "integer" },
                    "start_sec": { "type": "number", "description": "Region start in seconds" },
                    "end_sec": { "type": "number", "description": "Region end in seconds" }
                },
                "required": ["track", "start_sec", "end_sec"]
            }),
        )
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };
        if args.start_sec >= args.end_sec {
            return Ok(ToolResult::Error("start_sec must be < end_sec".into()));
        }
        let state = match load_head_state(ctx) {
            Ok(s) => s,
            Err(e) => return Ok(ToolResult::Error(e)),
        };
        if let Err(e) = check_track_index(&state.tracks, args.track) {
            return Ok(ToolResult::Error(e));
        }
        let clip = match state.tracks[args.track].clips.first() {
            Some(c) => c.clone(),
            None => {
                return Ok(ToolResult::Error(format!(
                    "track {} has no clips",
                    args.track
                )))
            }
        };
        let decoded = match audio_decoder::decode_file(&clip.source_path) {
            Ok(d) => d,
            Err(e) => return Ok(ToolResult::Error(format!("decode failed: {e}"))),
        };
        let sr = decoded.sample_rate;
        let channels = (decoded.channels as usize).max(1);
        let start_frame =
            ((args.start_sec * sr as f64) as usize).min(decoded.samples.len() / channels);
        let end_frame = ((args.end_sec * sr as f64) as usize).min(decoded.samples.len() / channels);
        let mono: Vec<f32> = (start_frame..end_frame)
            .map(|f| {
                (0..channels)
                    .map(|ch| decoded.samples[f * channels + ch])
                    .sum::<f32>()
                    / channels as f32
            })
            .collect();
        let magnitudes = compute_fft_magnitude(&mono, sr, FFT_SIZE);
        let bin_hz = sr as f32 / FFT_SIZE as f32;
        let points: Vec<serde_json::Value> = magnitudes
            .iter()
            .enumerate()
            .map(|(i, &db)| json!({ "hz": i as f32 * bin_hz, "db": db }))
            .collect();
        Ok(ToolResult::Ok(json!({
            "type": "spectrum",
            "track": args.track,
            "start_sec": args.start_sec,
            "end_sec": args.end_sec,
            "sample_rate": sr,
            "fft_size": FFT_SIZE,
            "points": points,
            "summary": format!("Spectrum for track {} ({:.2}s..{:.2}s), {} bins", args.track, args.start_sec, args.end_sec, magnitudes.len())
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::compute_fft_magnitude;

    #[test]
    fn sine_440hz_peak_near_440() {
        let sr = 44100u32;
        let samples: Vec<f32> = (0..sr)
            .map(|i| (2.0 * std::f32::consts::PI * 440.0 * i as f32 / sr as f32).sin())
            .collect();
        let bins = compute_fft_magnitude(&samples, sr, 4096);
        let peak_bin = bins
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap())
            .map(|(i, _)| i)
            .unwrap();
        let peak_freq = peak_bin as f32 * sr as f32 / 4096.0;
        assert!(
            (peak_freq - 440.0).abs() < 20.0,
            "peak at {peak_freq}Hz, expected ~440Hz"
        );
    }
}
