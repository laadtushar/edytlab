//! `apply_diff` — atomically emit N alternative branches off a parent
//! node by applying N distinct [`SessionDiff`]s (M24).
//!
//! This is the AI's primary "compare alternatives" surface: in one tool
//! call, the model authors three (or more) variants of "what should
//! the drop sound like" and the store appends three sibling nodes off
//! the parent in a single transactional update. The caller can then
//! `compare_nodes` and `revert_to` whichever variant wins.

use serde::Deserialize;
use serde_json::{json, Value};
use session::{NodeId, SessionDiff};

use crate::schema::anthropic_tool;
use crate::{Tool, ToolContext, ToolResult};

#[derive(Debug, Deserialize)]
struct BranchSpec {
    ops: SessionDiff,
    #[serde(default)]
    label: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Args {
    from_node: String,
    branches: Vec<BranchSpec>,
}

pub struct ApplyDiffTool;

impl Tool for ApplyDiffTool {
    fn name(&self) -> &'static str {
        "apply_diff"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "apply_diff",
            "Apply one or more SessionDiff specs to a parent node, producing one new sibling node per spec. All new nodes are written atomically: either every branch lands on disk or none do. Use to author N alternative takes in a single step.",
            json!({
                "type": "object",
                "properties": {
                    "from_node": { "type": "string", "description": "hex node id of parent" },
                    "branches": {
                        "type": "array",
                        "minItems": 1,
                        "items": {
                            "type": "object",
                            "properties": {
                                "ops": { "type": "object" },
                                "label": { "type": "string" }
                            },
                            "required": ["ops"],
                            "additionalProperties": false
                        }
                    }
                },
                "required": ["from_node", "branches"],
                "additionalProperties": false,
            }),
        )
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };

        let parent = match NodeId::from_hex(&args.from_node) {
            Ok(id) => id,
            Err(e) => return Ok(ToolResult::Error(format!("invalid node id: {e}"))),
        };

        let parent_state = match ctx.store.get(parent) {
            Ok(n) => n.state,
            Err(e) => return Ok(ToolResult::Error(format!("parent not found: {e}"))),
        };

        // Pre-compute every branch's resulting state. If ANY apply
        // fails, we abort before touching disk.
        let mut staged: Vec<(session::SessionState, Option<String>)> =
            Vec::with_capacity(args.branches.len());
        for (i, spec) in args.branches.iter().enumerate() {
            let mut s = parent_state.clone();
            if let Err(e) = spec.ops.apply(&mut s) {
                return Ok(ToolResult::Error(format!("branch {i} apply failed: {e}")));
            }
            staged.push((s, spec.label.clone()));
        }

        let ids = match ctx.store.append_branches(parent, staged) {
            Ok(ids) => ids,
            Err(e) => {
                return Ok(ToolResult::Error(format!(
                    "transactional append failed: {e}"
                )))
            }
        };

        Ok(ToolResult::Ok(json!({
            "branches": ids.iter().map(|id| id.to_hex()).collect::<Vec<_>>(),
            "summary": format!(
                "Authored {} branch{} off {}",
                ids.len(),
                if ids.len() == 1 { "" } else { "es" },
                args.from_node
            ),
        })))
    }
}
