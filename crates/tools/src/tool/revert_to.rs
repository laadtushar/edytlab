//! `revert_to` — append a node whose state matches `target`'s, parented
//! to the current head (M24). Preserves the full history; only head
//! moves.

use serde::Deserialize;
use serde_json::{json, Value};
use session::NodeId;

use crate::schema::anthropic_tool;
use crate::{Tool, ToolContext, ToolResult};

#[derive(Debug, Deserialize)]
struct Args {
    target: String,
}

pub struct RevertToTool;

impl Tool for RevertToTool {
    fn name(&self) -> &'static str {
        "revert_to"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "revert_to",
            "Append a new node whose state matches the target node's state, parented to the current head. Useful for an 'undo to checkpoint' UX without losing the intermediate history.",
            json!({
                "type": "object",
                "properties": {
                    "target": { "type": "string", "description": "hex id of the node whose state to revert to" }
                },
                "required": ["target"],
                "additionalProperties": false,
            }),
        )
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };

        let target = match NodeId::from_hex(&args.target) {
            Ok(id) => id,
            Err(e) => return Ok(ToolResult::Error(format!("invalid node id: {e}"))),
        };

        let new_id = match ctx.store.revert_to(target) {
            Ok(id) => id,
            Err(e) => return Ok(ToolResult::Error(format!("revert failed: {e}"))),
        };

        Ok(ToolResult::Ok(json!({
            "node_id": new_id.to_hex(),
            "reverted_to": target.to_hex(),
            "summary": format!(
                "Reverted to {}; new head {}",
                target.to_hex(),
                new_id.to_hex()
            ),
        })))
    }
}
