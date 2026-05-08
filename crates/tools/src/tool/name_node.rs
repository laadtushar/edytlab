//! `name_node` — set or replace `SessionNode::label` for a node (M24).
//! Metadata-only: does NOT change the content-addressed [`NodeId`].

use serde::Deserialize;
use serde_json::{json, Value};
use session::NodeId;

use crate::schema::anthropic_tool;
use crate::{Tool, ToolContext, ToolResult};

#[derive(Debug, Deserialize)]
struct Args {
    node_id: String,
    label: String,
}

pub struct NameNodeTool;

impl Tool for NameNodeTool {
    fn name(&self) -> &'static str {
        "name_node"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "name_node",
            "Set or replace the human-readable label on a session node. Metadata only; does NOT change the node id (which is content-hashed over state, not label).",
            json!({
                "type": "object",
                "properties": {
                    "node_id": { "type": "string", "description": "hex id of the node to label" },
                    "label": { "type": "string", "description": "new label; pass an empty string to clear" }
                },
                "required": ["node_id", "label"],
                "additionalProperties": false,
            }),
        )
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };

        let id = match NodeId::from_hex(&args.node_id) {
            Ok(id) => id,
            Err(e) => return Ok(ToolResult::Error(format!("invalid node id: {e}"))),
        };

        let label = if args.label.is_empty() {
            None
        } else {
            Some(args.label.clone())
        };

        if let Err(e) = ctx.store.set_label(id, label.clone()) {
            return Ok(ToolResult::Error(format!("set_label failed: {e}")));
        }

        Ok(ToolResult::Ok(json!({
            "node_id": id.to_hex(),
            "label": label,
            "summary": format!(
                "Labelled {} as {:?}",
                id.to_hex(),
                args.label
            ),
        })))
    }
}
