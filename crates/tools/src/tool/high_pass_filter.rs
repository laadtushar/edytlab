use crate::tool::util::CutoffFilter;
use crate::{Tool, ToolContext, ToolResult};
use serde_json::Value;

pub(crate) use audio_dsp::effects::high_pass_filter::apply_high_pass;

const FILTER: CutoffFilter = CutoffFilter {
    name: "high_pass_filter",
    description: "Apply a Butterworth high-pass filter to a track, removing frequencies below cutoff_hz. Appends a new session node.",
    apply: apply_high_pass,
};

pub struct HighPassFilterTool;

impl Tool for HighPassFilterTool {
    fn name(&self) -> &'static str {
        FILTER.name
    }

    fn schema(&self) -> Value {
        FILTER.schema()
    }

    // A pure edit of the head's audio and one node on top of it: safe on a
    // staged store, and safe to run again on a newer head (#421).
    fn runs_off_the_lock(&self) -> bool {
        true
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        FILTER.invoke(args, ctx)
    }
}
