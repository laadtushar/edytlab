use crate::schema::anthropic_tool;
use crate::tool::util::destructive_edit;
use crate::{Tool, ToolContext, ToolResult};
use serde::Deserialize;
use serde_json::Value;

const COMB_TUNING: [usize; 8] = [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617];
const ALLPASS_TUNING: [usize; 4] = [556, 441, 341, 225];
const STEREO_SPREAD: usize = 23;
const FIXED_GAIN: f32 = 0.015;

struct CombFilter {
    buf: Vec<f32>,
    idx: usize,
    feedback: f32,
    damp1: f32,
    damp2: f32,
    filterstore: f32,
}

impl CombFilter {
    fn new(size: usize, room: f32, damp: f32) -> Self {
        Self {
            buf: vec![0.0; size],
            idx: 0,
            feedback: room,
            damp1: damp,
            damp2: 1.0 - damp,
            filterstore: 0.0,
        }
    }

    fn process(&mut self, input: f32) -> f32 {
        let output = self.buf[self.idx];
        self.filterstore = output * self.damp2 + self.filterstore * self.damp1;
        self.buf[self.idx] = input + self.filterstore * self.feedback;
        self.idx = (self.idx + 1) % self.buf.len();
        output
    }
}

struct AllpassFilter {
    buf: Vec<f32>,
    idx: usize,
}

impl AllpassFilter {
    fn new(size: usize) -> Self {
        Self {
            buf: vec![0.0; size],
            idx: 0,
        }
    }

    fn process(&mut self, input: f32) -> f32 {
        let buf_out = self.buf[self.idx];
        let output = -input + buf_out;
        self.buf[self.idx] = input + buf_out * 0.5;
        self.idx = (self.idx + 1) % self.buf.len();
        output
    }
}

pub(crate) fn apply_reverb(
    samples: &mut [f32],
    sr: u32,
    channels: usize,
    room_size: f32,
    damping: f32,
    wet: f32,
) {
    let channels = channels.max(1);
    let scale = sr as f32 / 44100.0;
    let room = room_size.clamp(0.0, 1.0) * 0.28 + 0.7;
    let damp = damping.clamp(0.0, 1.0) * 0.4;
    let wet = wet.clamp(0.0, 1.0);
    let dry = 1.0 - wet;
    let n_frames = samples.len() / channels;
    let mut combs_l: Vec<CombFilter> = COMB_TUNING
        .iter()
        .map(|&t| CombFilter::new((t as f32 * scale) as usize, room, damp))
        .collect();
    let mut combs_r: Vec<CombFilter> = COMB_TUNING
        .iter()
        .map(|&t| CombFilter::new(((t + STEREO_SPREAD) as f32 * scale) as usize, room, damp))
        .collect();
    let mut allpasses_l: Vec<AllpassFilter> = ALLPASS_TUNING
        .iter()
        .map(|&t| AllpassFilter::new((t as f32 * scale) as usize))
        .collect();
    let mut allpasses_r: Vec<AllpassFilter> = ALLPASS_TUNING
        .iter()
        .map(|&t| AllpassFilter::new(((t + STEREO_SPREAD) as f32 * scale) as usize))
        .collect();
    for frame in 0..n_frames {
        let input: f32 = (0..channels)
            .map(|ch| samples[frame * channels + ch])
            .sum::<f32>()
            / channels as f32
            * FIXED_GAIN;
        let mut out_l = combs_l.iter_mut().map(|c| c.process(input)).sum::<f32>();
        let mut out_r = combs_r.iter_mut().map(|c| c.process(input)).sum::<f32>();
        for ap in &mut allpasses_l {
            out_l = ap.process(out_l);
        }
        for ap in &mut allpasses_r {
            out_r = ap.process(out_r);
        }
        if channels == 1 {
            samples[frame] = samples[frame] * dry + out_l * wet;
        } else {
            samples[frame * channels] = samples[frame * channels] * dry + out_l * wet;
            samples[frame * channels + 1] = samples[frame * channels + 1] * dry + out_r * wet;
            for ch in 2..channels {
                samples[frame * channels + ch] *= dry;
            }
        }
    }
}

#[derive(Debug, Deserialize)]
struct Args {
    track: usize,
    room_size: Option<f32>,
    damping: Option<f32>,
    wet: Option<f32>,
}

pub struct ReverbTool;

impl Tool for ReverbTool {
    fn name(&self) -> &'static str {
        "reverb"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "reverb",
            "Apply Freeverb algorithmic reverb. room_size (0-1) controls reverb length, damping (0-1) controls high-freq decay, wet (0-1) is the wet/dry blend. Appends a new session node.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "track": { "type": "integer" },
                    "room_size": { "type": "number", "default": 0.5, "description": "Room size 0..1" },
                    "damping": { "type": "number", "default": 0.5, "description": "High-freq damping 0..1" },
                    "wet": { "type": "number", "default": 0.3, "description": "Wet mix 0..1" },
                    "start_sec": { "type": "number" },
                    "end_sec": { "type": "number" }
                },
                "required": ["track"]
            }),
        )
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };
        let room = args.room_size.unwrap_or(0.5);
        let damp = args.damping.unwrap_or(0.5);
        let wet = args.wet.unwrap_or(0.3);
        let channels = {
            let state = match crate::tool::util::load_head_state(ctx) {
                Ok(s) => s,
                Err(e) => return Ok(ToolResult::Error(e)),
            };
            if let Err(e) = crate::tool::util::check_track_index(&state.tracks, args.track) {
                return Ok(ToolResult::Error(e));
            }
            let clip = state.tracks[args.track].clips.first().cloned();
            if let Some(c) = clip {
                audio_decoder::decode_file(&c.source_path)
                    .map(|d| d.channels as usize)
                    .unwrap_or(1)
            } else {
                return Ok(ToolResult::Error(format!(
                    "track {} has no clips",
                    args.track
                )));
            }
        };
        Ok(destructive_edit(
            ctx,
            args.track,
            move |samples, sr| {
                let mut v = samples.to_vec();
                apply_reverb(&mut v, sr, channels, room, damp, wet);
                *samples = v;
            },
            format!(
                "reverb track {} room={:.2} wet={:.2}",
                args.track, room, wet
            ),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::apply_reverb;

    #[test]
    fn wet_zero_passes_through() {
        let original = vec![0.5f32, -0.3, 0.1, 0.8, 0.0, -0.5];
        let mut samples = original.clone();
        apply_reverb(&mut samples, 44100, 1, 0.5, 0.5, 0.0);
        for (a, b) in original.iter().zip(samples.iter()) {
            assert!((a - b).abs() < 1e-5, "wet=0 should pass through unchanged");
        }
    }

    #[test]
    fn wet_one_returns_reverb_only() {
        let mut samples: Vec<f32> = (0..4410).map(|i| if i < 100 { 1.0 } else { 0.0 }).collect();
        apply_reverb(&mut samples, 44100, 1, 0.8, 0.5, 1.0);
        let tail_energy: f32 = samples[200..].iter().map(|s| s * s).sum();
        assert!(tail_energy > 0.001, "reverb tail should have energy");
    }
}
