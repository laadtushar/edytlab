use crate::tool::util::CutoffFilter;
use crate::{Tool, ToolContext, ToolResult};
use serde_json::Value;

pub(crate) use audio_dsp::effects::low_pass_filter::apply_low_pass;

const FILTER: CutoffFilter = CutoffFilter {
    name: "low_pass_filter",
    description: "Apply a Butterworth low-pass filter to a track, removing frequencies above cutoff_hz. Appends a new session node.",
    apply: apply_low_pass,
};

pub struct LowPassFilterTool;

impl Tool for LowPassFilterTool {
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
