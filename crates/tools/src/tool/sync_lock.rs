//! `set_sync_lock` — keep the tracks aligned through edits (#170 §3).
//!
//! Without it, cutting a sentence out of one speaker's track leaves
//! every later word on that track early by the length of the cut, while
//! the other speaker stays where they were. The conversation comes
//! apart, and nothing about the edit says it will.
//!
//! Sync-lock is the mode that says an edit to one track is an edit to
//! the timeline. `cut_range` and `insert_silence` read it; both apply
//! the same time change to every track in the *same* node, so undo puts
//! the whole thing back rather than half of it.
//!
//! ## Why it is a node
//!
//! It changes what a subsequent edit does, so a history that did not
//! record it could not explain its own results — a cut two nodes later
//! would have moved three tracks for no reason the DAG can show. It is
//! also how the mode survives Save As and how the agent can read it
//! before deciding whether a cut is safe.

use serde::Deserialize;
use serde_json::{json, Value};

use crate::schema::anthropic_tool;
use crate::tool::util::{append_state, load_head_state};
use crate::{Tool, ToolContext, ToolResult};

#[derive(Debug, Deserialize)]
struct Args {
    enabled: bool,
}

pub struct SetSyncLockTool;

impl Tool for SetSyncLockTool {
    fn name(&self) -> &'static str {
        "set_sync_lock"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "set_sync_lock",
            "Turn sync-lock on or off. With it on, an edit that shifts time on one track shifts \
             every track, so a multitrack recording stays aligned — cutting a sentence from one \
             speaker's track in an interview moves the other speaker's track by the same amount \
             rather than desynchronising the conversation. Affects `cut_range` and \
             `insert_silence`; each is still one undoable node covering every track it moved.",
            json!({
                "type": "object",
                "properties": {
                    "enabled": {
                        "type": "boolean",
                        "description": "True to keep tracks aligned through edits that shift time.",
                    },
                },
                "required": ["enabled"],
                "additionalProperties": false,
            }),
        )
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };

        let mut state = match load_head_state(ctx) {
            Ok(s) => s,
            Err(msg) => return Ok(ToolResult::Error(msg)),
        };

        if state.sync_lock == args.enabled {
            // Appending a node identical to the head would be a no-op
            // the user has to undo past. Saying so is more useful.
            return Ok(ToolResult::Ok(json!({
                "sync_lock": args.enabled,
                "changed": false,
                "summary": format!(
                    "Sync-lock is already {}.",
                    if args.enabled { "on" } else { "off" }
                ),
            })));
        }

        state.sync_lock = args.enabled;
        let tracks = state.tracks.len();

        let new_id = match append_state(
            ctx,
            state,
            format!("sync-lock {}", if args.enabled { "on" } else { "off" }),
        ) {
            Ok(id) => id,
            Err(msg) => return Ok(ToolResult::Error(msg)),
        };

        Ok(ToolResult::Ok(json!({
            "node_id": new_id.to_hex(),
            "sync_lock": args.enabled,
            "changed": true,
            "summary": if args.enabled {
                format!(
                    "Sync-lock on: cuts and inserts now move all {tracks} track(s) together, \
                     so the recording stays aligned."
                )
            } else {
                "Sync-lock off: edits affect only the track they name.".to_string()
            },
        })))
    }
}
