//! `revert_to` — move the head back to `target` (M24). Node ids hash the
//! state, so this lands on `target` itself rather than adding a node:
//! nothing is created, every node after it is kept, and only the head
//! moves (see `Store::revert_to`).

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
            "Move the head back to an earlier node, such as a checkpoint before an edit the user wants to undo. Nothing is deleted: every node after it is kept, so the user can come back to any of them.",
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

        // Its audio may have been swept from history; put it back first.
        if let Err(e) = crate::rederive::materialize(ctx.store, target) {
            return Ok(ToolResult::Error(e));
        }

        let new_id = match ctx.store.revert_to(target) {
            Ok(id) => id,
            Err(e) => return Ok(ToolResult::Error(format!("revert failed: {e}"))),
        };

        Ok(ToolResult::Ok(json!({
            "node_id": new_id.to_hex(),
            "reverted_to": target.to_hex(),
            "summary": format!(
                "Reverted to {}; head is now {}",
                target.to_hex(),
                new_id.to_hex()
            ),
        })))
    }
}
