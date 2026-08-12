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
            "Render a session node to a final audio file at the user's chosen path. \
             Supports format=\"wav\" (uncompressed) and format=\"flac\" (lossless, \
             roughly half the size and identical audio — prefer it when the user \
             wants to send the file somewhere). MP3 is not available yet.",
            json!({
                "type": "object",
                "properties": {
                    "node_id": { "type": "string" },
                    "format": { "type": "string", "enum": ["wav", "flac"] },
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
            "wav" | "flac" => {}
            // `mp3` is deliberately absent from the schema enum, so a
            // request for it is refused by schema validation before it
            // reaches here — which is the point: the tool should not
            // advertise a format it cannot produce. This arm catches
            // anything that slips past validation.
            other => {
                return Ok(ToolResult::Error(format!(
                    "unknown format {other:?}; supported: \"wav\", \"flac\""
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
        let want_flac = args.format == "flac";

        // FLAC renders via WAV and transcodes rather than through a
        // second render path. `render_streaming` writes incrementally
        // through `hound`, and giving it a parallel FLAC branch would
        // mean two copies of the mixdown loop — the duplication that
        // produced #80 and #81. The intermediate is lossless: both sides
        // are 16-bit PCM, so decoding gives back exactly the samples
        // that were written and re-encoding quantises them to the same
        // integers.
        let render_target = if want_flac {
            let tmp = match tempfile::Builder::new().suffix(".wav").tempfile() {
                Ok(t) => t,
                Err(e) => return Ok(ToolResult::Error(format!("temp file failed: {e}"))),
            };
            Some(tmp)
        } else {
            None
        };
        let render_path = render_target
            .as_ref()
            .map(|t| t.path().to_path_buf())
            .unwrap_or_else(|| out_path.clone());

        let report = match ctx.engine.render_to_wav(&node.state, &render_path, None) {
            Ok(r) => r,
            Err(e) => return Ok(ToolResult::Error(format!("render failed: {e}"))),
        };

        if want_flac {
            let decoded = match audio_decoder::decode_file(&render_path) {
                Ok(d) => d,
                Err(e) => {
                    return Ok(ToolResult::Error(format!(
                        "could not re-read the render for flac encoding: {e}"
                    )))
                }
            };
            if let Err(e) = audio_engine::write_flac(
                &decoded.samples,
                decoded.sample_rate,
                decoded.channels,
                &out_path,
            ) {
                return Ok(ToolResult::Error(format!("flac encoding failed: {e}")));
            }
        }

        Ok(ToolResult::Ok(json!({
            "path": out_path.to_string_lossy(),
            "frames_written": report.frames_written,
            "sample_rate": report.sample_rate,
            "channels": report.channels,
            "peak_dbfs": report.peak_dbfs,
            "format": args.format,
            "summary": format!(
                "Rendered final {} ({} frames, {} ch, {} Hz) to {}",
                args.format, report.frames_written, report.channels, report.sample_rate,
                out_path.display(),
            ),
        })))
    }
}
