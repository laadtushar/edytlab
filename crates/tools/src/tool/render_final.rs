//! `render_final` — render a node's session state to a user-chosen
//! path.
//!
//! Phase 1 supports `format = "wav"` only. The Anthropic schema lists
//! `mp3` and `flac` for forward compatibility (Phase 2 brings encoders
//! in), but invoking with those today returns an actionable error so
//! the model can fall back to wav.

use std::path::PathBuf;

use serde::Deserialize;
use serde_json::{json, Value};

use crate::schema::anthropic_tool;
use crate::{Tool, ToolContext, ToolResult};

#[derive(Debug, Deserialize)]
struct Args {
    node_id: String,
    format: String,
    out_path: String,
}

pub struct RenderFinalTool;

impl Tool for RenderFinalTool {
    fn name(&self) -> &'static str {
        "render_final"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "render_final",
            "Render a session node to a final audio file at the user's chosen path. Phase 1 supports format=\"wav\"; mp3 and flac return an actionable error.",
            json!({
                "type": "object",
                "properties": {
                    "node_id": { "type": "string" },
                    "format": { "type": "string", "enum": ["wav", "mp3", "flac"] },
                    "out_path": { "type": "string" },
                },
                "required": ["node_id", "format", "out_path"],
                "additionalProperties": false,
            }),
        )
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };

        match args.format.as_str() {
            "wav" => {}
            "mp3" | "flac" => {
                return Ok(ToolResult::Error(format!(
                    "format {:?} is not supported in Phase 1; use format=\"wav\"",
                    args.format
                )));
            }
            other => {
                return Ok(ToolResult::Error(format!(
                    "unknown format {other:?}; supported: \"wav\""
                )));
            }
        }

        let node_id = match session::NodeId::from_hex(&args.node_id) {
            Ok(id) => id,
            Err(e) => return Ok(ToolResult::Error(format!("invalid node_id: {e}"))),
        };

        let node = match ctx.store.get(node_id) {
            Ok(n) => n,
            Err(e) => return Ok(ToolResult::Error(format!("node lookup failed: {e}"))),
        };

        let out_path = PathBuf::from(&args.out_path);
        let report = match ctx.engine.render_to_wav(&node.state, &out_path, None) {
            Ok(r) => r,
            Err(e) => return Ok(ToolResult::Error(format!("render failed: {e}"))),
        };

        Ok(ToolResult::Ok(json!({
            "path": out_path.to_string_lossy(),
            "frames_written": report.frames_written,
            "sample_rate": report.sample_rate,
            "channels": report.channels,
            "peak_dbfs": report.peak_dbfs,
            "summary": format!(
                "Rendered final ({} frames, {} ch, {} Hz) to {}",
                report.frames_written, report.channels, report.sample_rate,
                out_path.display(),
            ),
        })))
    }
}
