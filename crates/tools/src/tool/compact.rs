//! `compact_session` — trade history for disk, deliberately (#98).
//!
//! The sweep in [`crate::reclaim`] is a cache policy: it only removes
//! what a replay can put back, so it needs no permission and loses
//! nothing. This is the other half, and it is a different kind of
//! thing entirely — it **deletes history**. Nodes go, and the undo
//! steps they represented go with them.
//!
//! So it is a verb the user says, never a background policy, and it
//! behaves accordingly:
//!
//! * **Dry run by default.** It reports what it would remove and
//!   changes nothing until asked twice. The same shape as
//!   `remove_fillers`, and for the same reason: a destructive sweep
//!   across a whole session is not something to discover afterwards.
//! * **The head's chain is never pruned.** Undo along the line you are
//!   actually on keeps working; what goes is the abandoned branches and
//!   the tail beyond `keep_last`.
//! * **Audio goes only after the nodes do.** A file is removed only
//!   once nothing left in the store names it, so a half-finished
//!   compaction leaves a session that still works rather than nodes
//!   pointing at deleted audio.

use serde::Deserialize;
use serde_json::{json, Value};

use crate::schema::anthropic_tool;
use crate::{Tool, ToolContext, ToolResult};

/// How much history to keep by default: enough that ordinary undo is
/// untouched, few enough that a long session actually shrinks.
const DEFAULT_KEEP_LAST: usize = 20;

#[derive(Debug, Deserialize)]
struct Args {
    /// Nodes to keep on the head's chain, newest first.
    #[serde(default)]
    keep_last: Option<usize>,
    /// Actually do it. Absent or false reports and changes nothing.
    #[serde(default)]
    apply: bool,
}

pub struct CompactSessionTool;

impl Tool for CompactSessionTool {
    fn name(&self) -> &'static str {
        "compact_session"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "compact_session",
            "Prune old history and delete the audio only it referenced, to reclaim disk. This \
             removes undo steps permanently — the nodes are gone, not archived. Reports what it \
             would remove and changes nothing unless `apply` is true. The head's most recent \
             `keep_last` nodes are never pruned, so ordinary undo keeps working; what goes is \
             the tail beyond that and any abandoned branches. This is currently the only way \
             to reclaim derived audio: nothing sweeps the cache in the background, so do not \
             tell the user to wait for one. Run `storage_report` first to see what is actually \
             using the space.",
            json!({
                "type": "object",
                "properties": {
                    "keep_last": {
                        "type": "integer",
                        "minimum": 1,
                        "description":
                            "How many recent nodes on the head's chain to keep. Default 20.",
                    },
                    "apply": {
                        "type": "boolean",
                        "description":
                            "True to actually prune. Omit to see what would go without doing it.",
                    },
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
        let keep_last = args.keep_last.unwrap_or(DEFAULT_KEEP_LAST).max(1);

        let Some(head) = ctx.store.head() else {
            return Ok(ToolResult::Error(
                "no session loaded; there is nothing to compact".to_string(),
            ));
        };

        let plan = match crate::reclaim::plan_compaction(ctx.store, head, keep_last) {
            Ok(p) => p,
            Err(e) => return Ok(ToolResult::Error(e)),
        };

        if plan.nodes.is_empty() {
            return Ok(ToolResult::Ok(json!({
                "applied": false,
                "prunable_nodes": 0,
                "reclaimable_bytes": 0,
                "summary": format!(
                    "Nothing to compact: the session has {} node(s) on and off the head's chain, \
                     which is within the last {keep_last}.",
                    plan.total_nodes
                ),
            })));
        }

        if !args.apply {
            return Ok(ToolResult::Ok(json!({
                "applied": false,
                "prunable_nodes": plan.nodes.len(),
                "reclaimable_bytes": plan.bytes,
                "keep_last": keep_last,
                "summary": format!(
                    "Would remove {} node(s) of history and {:.1} MB of audio, keeping the last \
                     {keep_last} on the current chain. Those undo steps would be gone for good — \
                     call again with apply: true to do it.",
                    plan.nodes.len(),
                    plan.bytes as f64 / (1024.0 * 1024.0),
                ),
            })));
        }

        let report = match crate::reclaim::apply_compaction(ctx.store, &plan) {
            Ok(r) => r,
            Err(e) => return Ok(ToolResult::Error(e)),
        };

        Ok(ToolResult::Ok(json!({
            "applied": true,
            "removed_nodes": report.removed_nodes,
            "removed_files": report.removed_files,
            "freed_bytes": report.freed_bytes,
            "summary": format!(
                "Removed {} node(s) of history and {} file(s), freeing {:.1} MB. The last \
                 {keep_last} nodes on the current chain are untouched, so recent undo still works.",
                report.removed_nodes,
                report.removed_files,
                report.freed_bytes as f64 / (1024.0 * 1024.0),
            ),
        })))
    }
}
