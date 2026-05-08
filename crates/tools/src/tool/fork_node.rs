//! `fork_node` — branch the session DAG at a chosen node (M24).
//!
//! Defaults to the current head when `from` is omitted. Sets head to
//! the chosen node so subsequent edits become a new branch off it. The
//! id returned is the same as `from` — content addressing means a
//! byte-identical "duplicate" shares the parent's id. Distinct branch
//! nodes only exist after the next mutating tool runs on this branch.

use serde::Deserialize;
use serde_json::{json, Value};
use session::NodeId;

use crate::schema::anthropic_tool;
use crate::{Tool, ToolContext, ToolResult};

#[derive(Debug, Deserialize)]
struct Args {
    #[serde(default)]
    from: Option<String>,
}

pub struct ForkNodeTool;

impl Tool for ForkNodeTool {
    fn name(&self) -> &'static str {
        "fork_node"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "fork_node",
            "Branch the session DAG at the given node id (defaults to the current head when `from` is omitted). Sets head to that node so subsequent mutating tools form a new branch parented off it. Returns the branch-point node id.",
            json!({
                "type": "object",
                "properties": {
                    "from": { "type": "string", "description": "hex node id; defaults to current head" }
                },
                "required": [],
                "additionalProperties": false,
            }),
        )
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };

        let target = match args.from {
            Some(hex) => match NodeId::from_hex(&hex) {
                Ok(id) => id,
                Err(e) => return Ok(ToolResult::Error(format!("invalid node id: {e}"))),
            },
            None => match ctx.store.head() {
                Some(id) => id,
                None => {
                    return Ok(ToolResult::Error(
                        "no session loaded; call `load` first".into(),
                    ))
                }
            },
        };

        let id = match ctx.store.fork(target) {
            Ok(id) => id,
            Err(e) => return Ok(ToolResult::Error(format!("fork failed: {e}"))),
        };

        Ok(ToolResult::Ok(json!({
            "node_id": id.to_hex(),
            "summary": format!(
                "Forked at {}; subsequent edits will branch off this node",
                id.to_hex()
            ),
        })))
    }
}
