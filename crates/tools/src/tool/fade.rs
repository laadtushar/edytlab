//! Fade-in / fade-out within a region, with an optional gain curve.

use crate::schema::anthropic_tool;
use crate::tool::util::{destructive_edit, track_channels};
use crate::util::range_resolver::{resolve as resolve_range, RangeError};
use crate::{Range, Tool, ToolContext, ToolResult};
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Clone, Copy)]
pub enum Kind {
    In,
    Out,
}

/// Shape the gain without changing the fade's frame grid or channel layout.
#[derive(Debug, Default, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Curve {
    #[default]
    Linear,
    Exponential,
    Logarithmic,
    SCurve,
}

impl Curve {
    fn gain(self, position: f32) -> f32 {
        match self {
            Self::Linear => position,
            Self::Exponential => position * position,
            Self::Logarithmic => position.sqrt(),
            Self::SCurve => position * position * (3.0 - 2.0 * position),
        }
    }
}

/// Ramp `[range.start_sec, range.end_sec)` in or out.
///
/// `channels` is the interleave stride. Seconds convert to *frames*,
/// and the gain is computed per frame and applied to every channel of
/// it — indexing the interleaved buffer as if it were mono would both
/// halve the ramp's duration on stereo and give the two channels
/// slightly different gains at each step.
pub fn apply_fade(
    samples: &mut [f32],
    sample_rate: u32,
    channels: usize,
    range: Range,
    kind: Kind,
) {
    apply_fade_with_curve(samples, sample_rate, channels, range, kind, Curve::Linear);
}

/// Apply a curve on the legacy half-open frame grid: `t = offset / N`.
///
/// Fade-out evaluates the same curve at `1 - t`. The excluded end frame
/// would have `t = 1`; the last edited frame has `t = (N - 1) / N`.
/// Keeping this grid and f32 arithmetic preserves old linear audio hashes.
pub fn apply_fade_with_curve(
    samples: &mut [f32],
    sample_rate: u32,
    channels: usize,
    range: Range,
    kind: Kind,
    curve: Curve,
) {
    let stride = channels.max(1);
    let total_frames = samples.len() / stride;
    let start = ((range.start_sec * sample_rate as f64) as usize).min(total_frames);
    let end = ((range.end_sec * sample_rate as f64) as usize).min(total_frames);
    if end <= start {
        return;
    }
    let len = (end - start) as f32;
    for frame in start..end {
        let t = (frame - start) as f32 / len;
        let position = match kind {
            Kind::In => t,
            Kind::Out => 1.0 - t,
        };
        let gain = curve.gain(position);
        for ch in 0..stride {
            samples[frame * stride + ch] *= gain;
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct FadeParams {
    pub range: Option<Range>,
    #[serde(default = "default_kind")]
    pub kind: KindParam,
    #[serde(default)]
    pub curve: Curve,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KindParam {
    In,
    Out,
}

fn default_kind() -> KindParam {
    KindParam::Out
}

impl From<KindParam> for Kind {
    fn from(p: KindParam) -> Self {
        match p {
            KindParam::In => Kind::In,
            KindParam::Out => Kind::Out,
        }
    }
}

pub fn dispatch_fade(
    params: FadeParams,
    user_message: &str,
    samples: &mut [f32],
    sample_rate: u32,
    channels: usize,
) -> Result<(), FadeError> {
    let range = resolve_range(params.range, user_message, true)
        .map_err(FadeError::Range)?
        .expect("required => Some on Ok");
    apply_fade_with_curve(
        samples,
        sample_rate,
        channels,
        range,
        params.kind.into(),
        params.curve,
    );
    Ok(())
}

#[derive(Debug, thiserror::Error)]
pub enum FadeError {
    #[error("{0}")]
    Range(#[from] RangeError),
}

// ---------------------------------------------------------------------------
// Tool trait impl
// ---------------------------------------------------------------------------

pub struct FadeTool;

impl Tool for FadeTool {
    fn name(&self) -> &'static str {
        "fade"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "fade",
            "Apply a fade-in or fade-out to a time range of a track. \
             Requires a range (start_sec, end_sec). Kind defaults to 'out'; curve to 'linear'. \
             Appends a new session node.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "track": { "type": "integer" },
                    "range": {
                        "type": "object",
                        "properties": {
                            "start_sec": { "type": "number" },
                            "end_sec": { "type": "number" }
                        },
                        "required": ["start_sec", "end_sec"]
                    },
                    "kind": { "type": "string", "enum": ["in", "out"] },
                    "curve": {
                        "type": "string",
                        "enum": ["linear", "exponential", "logarithmic", "s_curve"],
                        "default": "linear",
                        "description": "Gain at t: linear t, exponential t*t, logarithmic sqrt(t), s_curve t*t*(3-2*t). Fade-out uses the same curve at 1-t. Defaults to linear."
                    }
                },
                "required": ["track"],
                "additionalProperties": false
            }),
        )
    }

    // A pure edit of the head's audio and one node on top of it: safe on a
    // staged store, and safe to run again on a newer head (#421).
    fn runs_off_the_lock(&self) -> bool {
        true
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        #[derive(serde::Deserialize)]
        struct Args {
            track: usize,
            range: Option<Range>,
            #[serde(default = "default_kind")]
            kind: KindParam,
            #[serde(default)]
            curve: Curve,
        }

        let parsed: Args = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };

        let range = match crate::util::range_resolver::resolve(parsed.range, ctx.user_message, true)
        {
            Ok(Some(r)) => r,
            Ok(None) => unreachable!("required=true"),
            Err(e) => return Ok(ToolResult::Error(e.to_string())),
        };

        let kind: Kind = parsed.kind.into();
        let track = parsed.track;
        let channels = match track_channels(ctx, track) {
            Ok(c) => c,
            Err(e) => return Ok(ToolResult::Error(e)),
        };
        Ok(destructive_edit(
            ctx,
            track,
            move |samples, sample_rate| {
                apply_fade_with_curve(samples, sample_rate, channels, range, kind, parsed.curve);
            },
            format!(
                "fade {:?} {:.2}s\u{2013}{:.2}s on track {}",
                kind, range.start_sec, range.end_sec, track
            ),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::{Curve, Kind};

    #[test]
    fn curve_gains_at_endpoints_and_quartiles_for_both_kinds() {
        let cases = [
            (Curve::Linear, [0.0, 0.25, 0.5, 0.75, 1.0]),
            (Curve::Exponential, [0.0, 0.0625, 0.25, 0.5625, 1.0]),
            (
                Curve::Logarithmic,
                [0.0, 0.5, std::f32::consts::FRAC_1_SQRT_2, 0.8660254, 1.0],
            ),
            (Curve::SCurve, [0.0, 0.15625, 0.5, 0.84375, 1.0]),
        ];
        for (curve, expected) in cases {
            for kind in [Kind::In, Kind::Out] {
                for (i, t) in [0.0, 0.25, 0.5, 0.75, 1.0].into_iter().enumerate() {
                    let (position, index) = match kind {
                        Kind::In => (t, i),
                        Kind::Out => (1.0 - t, 4 - i),
                    };
                    assert_eq!(
                        curve.gain(position),
                        expected[index],
                        "{curve:?} {kind:?} {t}"
                    );
                }
            }
        }
    }
}
