//! `render_preview` — render a node's session state to a temp WAV.
//!
//! Does NOT mutate the session graph; the rendered file is meant to be
//! played back or inspected. The temp file is materialised inside the
//! OS tempdir and its path returned to the caller; the caller (or the
//! agent) is responsible for cleanup. We intentionally do not return a
//! `tempfile::TempPath` because the `ToolResult` payload crosses a
//! JSON boundary.

use std::path::PathBuf;

use serde::Deserialize;
use serde_json::{json, Value};

use crate::schema::anthropic_tool;
use crate::{Tool, ToolContext, ToolResult};

#[derive(Debug, Deserialize)]
struct Args {
    node_id: String,
    #[serde(default)]
    range: Option<[u64; 2]>,
}

pub struct RenderPreviewTool;

impl Tool for RenderPreviewTool {
    fn name(&self) -> &'static str {
        "render_preview"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "render_preview",
            "Render a session node to a temporary WAV file and return its path. Does not create a new session node. Optional `range` is a [start_sample, end_sample) pair into the rendered output.",
            json!({
                "type": "object",
                "properties": {
                    "node_id": { "type": "string" },
                    "range": {
                        "type": "array",
                        "items": { "type": "integer", "minimum": 0 },
                        "minItems": 2,
                        "maxItems": 2,
                    },
                },
                "required": ["node_id"],
                "additionalProperties": false,
            }),
        )
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };

        let node_id = match session::NodeId::from_hex(&args.node_id) {
            Ok(id) => id,
            Err(e) => return Ok(ToolResult::Error(format!("invalid node_id: {e}"))),
        };

        let node = match ctx.store.get(node_id) {
            Ok(n) => n,
            Err(e) => return Ok(ToolResult::Error(format!("node lookup failed: {e}"))),
        };

        let range = args.range.map(|[start, end]| audio_engine::TimeRange {
            start_frame: start,
            end_frame: end,
        });
        if let Some(r) = range {
            if r.start_frame >= r.end_frame {
                return Ok(ToolResult::Error(format!(
                    "invalid range: start ({}) must be < end ({})",
                    r.start_frame, r.end_frame
                )));
            }
        }

        // Use a named temp file inside the OS tempdir; persist the path
        // string so callers can reach it.
        let tmp = match tempfile::Builder::new()
            .prefix("edytlab-preview-")
            .suffix(".wav")
            .tempfile()
        {
            Ok(t) => t,
            Err(e) => return Ok(ToolResult::Error(format!("tempfile creation failed: {e}"))),
        };
        // Keep the file on disk after the handle drops; the caller now
        // owns cleanup. If `keep()` fails (permissions, disk full, ...) the
        // path we'd return would point at a file the tempfile guard already
        // removed, so surface the error instead of silently lying.
        let out_path: PathBuf = match tmp.into_temp_path().keep() {
            Ok(p) => p,
            Err(e) => {
                return Ok(ToolResult::Error(format!(
                    "failed to persist preview tempfile: {e}"
                )))
            }
        };

        let report = match ctx.engine.render_to_wav(&node.state, &out_path, range) {
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
                "Rendered preview ({} frames, {} ch, {} Hz) to {}",
                report.frames_written, report.channels, report.sample_rate,
                out_path.display(),
            ),
        })))
    }
}
