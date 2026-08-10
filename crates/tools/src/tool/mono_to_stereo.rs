use crate::schema::anthropic_tool;
use crate::tool::util::destructive_edit_rechannel;
use crate::{Tool, ToolContext, ToolResult};
use serde::Deserialize;
use serde_json::Value;

pub(crate) fn apply_mono_to_stereo(samples: &[f32]) -> Vec<f32> {
    let mut out = Vec::with_capacity(samples.len() * 2);
    for &s in samples {
        out.push(s);
        out.push(s);
    }
    out
}

#[derive(Debug, Deserialize)]
struct Args {
    track: usize,
}

pub struct MonoToStereoTool;

impl Tool for MonoToStereoTool {
    fn name(&self) -> &'static str {
        "mono_to_stereo"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "mono_to_stereo",
            "Convert a mono track to stereo by duplicating the channel to both L and R. Appends a new session node.",
            serde_json::json!({
                "type": "object",
                "properties": { "track": { "type": "integer" } },
                "required": ["track"]
            }),
        )
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };
        Ok(destructive_edit_rechannel(
            ctx,
            args.track,
            |samples, _sr, _ch| {
                let stereo = apply_mono_to_stereo(samples);
                *samples = stereo;
                // Two channels now — writing a mono header here would
                // stretch the track to twice its length, an octave low.
                2
            },
            format!("mono_to_stereo track {}", args.track),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::apply_mono_to_stereo;

    #[test]
    fn duplicates_channel() {
        let mono = vec![0.5f32, -0.3];
        let stereo = apply_mono_to_stereo(&mono);
        assert_eq!(stereo, vec![0.5, 0.5, -0.3, -0.3]);
    }
}
