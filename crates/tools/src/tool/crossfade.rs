//! Non-destructive crossfade over the shared timeline interval of two clips.
use serde::Deserialize;
use serde_json::{json, Value};
use session::{Clip, EnvelopePoint};

use crate::schema::anthropic_tool;
use crate::tool::util::{
    append_state, load_head_state, parse_args, seconds_to_frames, track_clip_rate,
};
use crate::{Tool, ToolContext, ToolResult};

#[derive(Debug, Default, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Curve {
    #[default]
    EqualPower,
    EqualGain,
}

impl Curve {
    fn gains(self, t: f64) -> (f64, f64) {
        match self {
            Self::EqualPower => {
                let angle = t * std::f64::consts::FRAC_PI_2;
                (angle.cos(), angle.sin())
            }
            Self::EqualGain => (1.0 - t, t),
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Args {
    from_track: usize,
    from_clip: usize,
    to_track: usize,
    to_clip: usize,
    #[serde(default)]
    curve: Curve,
}

/// The renderer interpolates dB, so sample the amplitude law into ordinary
/// envelope points. 128 segments include the quartiles and midpoint exactly.
/// -60 dB is the lane editor's floor; finite endpoints remain draggable.
fn envelope(
    clip: &Clip,
    rate: f64,
    start: f64,
    end: f64,
    curve: Curve,
    incoming: bool,
) -> Result<Vec<EnvelopePoint>, String> {
    let first = seconds_to_frames(start, rate).saturating_sub(clip.start_in_track);
    let last = seconds_to_frames(end, rate)
        .saturating_sub(clip.start_in_track)
        .min(clip.length);
    if last <= first {
        return Err("overlap must span at least one frame on both clips".into());
    }
    let segments = (last - first).min(128);
    Ok((0..=segments)
        .map(|i| {
            // Divide before adding: every point stays between first and last.
            let frame = first + ((last - first) as u128 * i as u128 / segments as u128) as u64;
            let t = (frame - first) as f64 / (last - first) as f64;
            let (out_gain, in_gain) = curve.gains(t);
            let gain = if incoming { in_gain } else { out_gain };
            EnvelopePoint {
                time_samples: frame,
                gain_db: (20.0 * gain.max(0.001).log10()) as f32,
            }
        })
        .collect())
}

pub struct CrossfadeTool;

impl Tool for CrossfadeTool {
    fn name(&self) -> &'static str {
        "crossfade"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "crossfade",
            "Crossfade two overlapping clips on one or two tracks over their shared session interval. Replaces both clip envelopes with editable curves; does not move clips or render audio. Equal-power default; equal-gain for correlated material. Position clips to overlap first. Endpoints use the lane's -60 dB floor.",
            json!({
                "type": "object",
                "properties": {
                    "from_track": { "type": "integer", "minimum": 0 },
                    "from_clip": { "type": "integer", "minimum": 0 },
                    "to_track": { "type": "integer", "minimum": 0 },
                    "to_clip": { "type": "integer", "minimum": 0 },
                    "curve": { "type": "string", "enum": ["equal_power", "equal_gain"], "default": "equal_power" }
                },
                "required": ["from_track", "from_clip", "to_track", "to_clip"],
                "additionalProperties": false
            }),
        )
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match parse_args(args) {
            Ok(a) => a,
            Err(r) => return Ok(r),
        };
        if (args.from_track, args.from_clip) == (args.to_track, args.to_clip) {
            return Ok(ToolResult::Error(
                "crossfade needs two distinct clips".into(),
            ));
        }
        let mut state = match load_head_state(ctx) {
            Ok(s) => s,
            Err(msg) => return Ok(ToolResult::Error(msg)),
        };
        let mut targets = Vec::with_capacity(2);
        for (track_index, clip_index) in [
            (args.from_track, args.from_clip),
            (args.to_track, args.to_clip),
        ] {
            let Some(track) = state.tracks.get(track_index) else {
                return Ok(ToolResult::Error(format!(
                    "track {track_index} out of range"
                )));
            };
            let Some(clip) = track.clips.get(clip_index) else {
                return Ok(ToolResult::Error(format!(
                    "clip {clip_index} out of range on track {track_index}"
                )));
            };
            let rate = match track_clip_rate(track, state.sample_rate) {
                Ok(r) => r,
                Err(msg) => return Ok(ToolResult::Error(msg)),
            };
            targets.push((clip, rate));
        }
        let start_sec = targets
            .iter()
            .map(|(c, r)| c.start_in_track as f64 / r)
            .fold(0.0, f64::max);
        let end_sec = targets
            .iter()
            .map(|(c, r)| c.start_in_track.saturating_add(c.length) as f64 / r)
            .fold(f64::INFINITY, f64::min);
        if end_sec <= start_sec {
            return Ok(ToolResult::Error(
                "clips must overlap in session time; position them first".into(),
            ));
        }
        // Validate and build both envelopes before changing either clip.
        let mut points = Vec::with_capacity(2);
        for (i, (clip, rate)) in targets.iter().enumerate() {
            match envelope(clip, *rate, start_sec, end_sec, args.curve, i == 1) {
                Ok(p) => points.push(p),
                Err(msg) => return Ok(ToolResult::Error(msg)),
            }
        }
        let incoming = points.pop().expect("two envelopes");
        let outgoing = points.pop().expect("two envelopes");
        state.tracks[args.from_track].clips[args.from_clip].set_envelope(outgoing);
        state.tracks[args.to_track].clips[args.to_clip].set_envelope(incoming);
        let id = match append_state(
            ctx,
            state,
            format!(
                "crossfade {}:{} to {}:{}",
                args.from_track, args.from_clip, args.to_track, args.to_clip
            ),
        ) {
            Ok(id) => id,
            Err(msg) => return Ok(ToolResult::Error(msg)),
        };
        Ok(ToolResult::Ok(json!({
            "node_id": id.to_hex(), "start_sec": start_sec, "end_sec": end_sec,
            "summary": "Crossfade written as editable clip envelopes"
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::Curve;

    #[test]
    fn gains_at_endpoints_and_quartiles() {
        let power = [
            (1.0, 0.0),
            (0.9238795325, 0.3826834324),
            (
                std::f64::consts::FRAC_1_SQRT_2,
                std::f64::consts::FRAC_1_SQRT_2,
            ),
            (0.3826834324, 0.9238795325),
            (0.0, 1.0),
        ];
        for (i, (out, incoming)) in power.into_iter().enumerate() {
            let t = i as f64 / 4.0;
            let (a, b) = Curve::EqualPower.gains(t);
            assert!((a - out).abs() < 1e-9 && (b - incoming).abs() < 1e-9);
            assert!((a * a + b * b - 1.0).abs() < 1e-12);
            assert_eq!(Curve::EqualGain.gains(t), (1.0 - t, t));
        }
    }
}
