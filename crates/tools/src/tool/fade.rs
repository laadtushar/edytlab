//! Linear fade-in / fade-out within a region.

use crate::schema::anthropic_tool;
use crate::tool::util::destructive_edit;
use crate::util::range_resolver::{resolve as resolve_range, RangeError};
use crate::{Range, Tool, ToolContext, ToolResult};
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Clone, Copy)]
pub enum Kind {
    In,
    Out,
}

pub fn apply_fade(samples: &mut [f32], sample_rate: u32, range: Range, kind: Kind) {
    let start = (range.start_sec * sample_rate as f64) as usize;
    let end = ((range.end_sec * sample_rate as f64) as usize).min(samples.len());
    if end <= start {
        return;
    }
    let len = (end - start) as f32;
    for (i, sample) in samples[start..end].iter_mut().enumerate() {
        let t = i as f32 / len;
        let gain = match kind {
            Kind::In => t,
            Kind::Out => 1.0 - t,
        };
        *sample *= gain;
    }
}

#[derive(Debug, Deserialize)]
pub struct FadeParams {
    pub range: Option<Range>,
    #[serde(default = "default_kind")]
    pub kind: KindParam,
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
) -> Result<(), FadeError> {
    let range = resolve_range(params.range, user_message, true)
        .map_err(FadeError::Range)?
        .expect("required => Some on Ok");
    apply_fade(samples, sample_rate, range, params.kind.into());
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
            "Apply a linear fade-in or fade-out to a time range of a track. \
             Requires a range (start_sec, end_sec). Kind defaults to 'out'. \
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
                    "kind": { "type": "string", "enum": ["in", "out"] }
                },
                "required": ["track"],
                "additionalProperties": false
            }),
        )
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        #[derive(serde::Deserialize)]
        struct Args {
            track: usize,
            range: Option<Range>,
            #[serde(default = "default_kind")]
            kind: KindParam,
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
        Ok(destructive_edit(
            ctx,
            track,
            move |samples, sample_rate| {
                apply_fade(samples, sample_rate, range, kind);
            },
            format!(
                "fade {:?} {:.2}s\u{2013}{:.2}s on track {}",
                kind, range.start_sec, range.end_sec, track
            ),
        ))
    }
}
