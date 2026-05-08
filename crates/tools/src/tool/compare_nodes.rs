//! `compare_nodes` — emit a structured [`SessionDiff`] between two
//! nodes (M24). Read-only: no head update, no new nodes.

use serde::Deserialize;
use serde_json::{json, Value};
use session::NodeId;

use crate::schema::anthropic_tool;
use crate::{Tool, ToolContext, ToolResult};

#[derive(Debug, Deserialize)]
struct Args {
    a: String,
    b: String,
}

pub struct CompareNodesTool;

impl Tool for CompareNodesTool {
    fn name(&self) -> &'static str {
        "compare_nodes"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "compare_nodes",
            "Compute the structural diff between two session nodes. Returns a `SessionDiff` JSON object with `added`, `removed`, and `modified` arrays of operations; each op identifies its target (track id, effect index, etc.) so callers can detect overlaps. Read-only.",
            json!({
                "type": "object",
                "properties": {
                    "a": { "type": "string", "description": "hex id of base node" },
                    "b": { "type": "string", "description": "hex id of target node" }
                },
                "required": ["a", "b"],
                "additionalProperties": false,
            }),
        )
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };

        let a = match NodeId::from_hex(&args.a) {
            Ok(id) => id,
            Err(e) => return Ok(ToolResult::Error(format!("invalid node id `a`: {e}"))),
        };
        let b = match NodeId::from_hex(&args.b) {
            Ok(id) => id,
            Err(e) => return Ok(ToolResult::Error(format!("invalid node id `b`: {e}"))),
        };

        let diff = match ctx.store.diff(a, b) {
            Ok(d) => d,
            Err(e) => return Ok(ToolResult::Error(format!("diff failed: {e}"))),
        };

        let diff_json = match serde_json::to_value(&diff) {
            Ok(v) => v,
            Err(e) => return Ok(ToolResult::Error(format!("diff serialise failed: {e}"))),
        };

        Ok(ToolResult::Ok(json!({
            "diff": diff_json,
            "summary": format!(
                "{} added, {} removed, {} modified",
                diff.added.len(),
                diff.removed.len(),
                diff.modified.len(),
            ),
        })))
    }
}
