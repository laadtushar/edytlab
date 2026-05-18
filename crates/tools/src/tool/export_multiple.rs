use serde::Deserialize;
use serde_json::{json, Value};

use crate::schema::anthropic_tool;
use crate::tool::util::load_head_state;
use crate::{Tool, ToolContext, ToolResult};

#[derive(Debug, Deserialize)]
struct Args {
    track_indices: Vec<usize>,
    output_dir: String,
    format: Option<String>,
}

pub struct ExportMultipleTool;

/// Sanitize a track name for use as a filename component.
/// Keeps alphanumeric chars, underscores, and hyphens; replaces everything
/// else (spaces, dots, slashes, etc.) with `_`.
fn sanitize_name(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

impl Tool for ExportMultipleTool {
    fn name(&self) -> &'static str {
        "export_multiple"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "export_multiple",
            "Export selected tracks as individual WAV files to a directory. Does not modify the session. Returns list of exported file paths.",
            json!({
                "type": "object",
                "properties": {
                    "track_indices": {
                        "type": "array",
                        "items": { "type": "integer" },
                        "description": "Indices of tracks to export"
                    },
                    "output_dir": {
                        "type": "string",
                        "description": "Directory to write exported files into"
                    },
                    "format": {
                        "type": "string",
                        "enum": ["wav"],
                        "default": "wav"
                    }
                },
                "required": ["track_indices", "output_dir"]
            }),
        )
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };

        // Validate format field.
        if let Some(ref fmt) = args.format {
            if fmt != "wav" {
                return Ok(ToolResult::Error(
                    "only 'wav' format is currently supported".to_string(),
                ));
            }
        }

        if args.track_indices.is_empty() {
            return Ok(ToolResult::Error(
                "track_indices must not be empty".to_string(),
            ));
        }

        let state = match load_head_state(ctx) {
            Ok(s) => s,
            Err(e) => return Ok(ToolResult::Error(e)),
        };

        // Validate all indices before doing any work.
        for &idx in &args.track_indices {
            if idx >= state.tracks.len() {
                return Ok(ToolResult::Error(format!(
                    "track {idx} is out of range; session has {} track{}",
                    state.tracks.len(),
                    if state.tracks.len() == 1 { "" } else { "s" },
                )));
            }
        }

        // Ensure output directory exists.
        let out_dir = std::path::Path::new(&args.output_dir);
        if let Err(e) = std::fs::create_dir_all(out_dir) {
            return Ok(ToolResult::Error(format!(
                "failed to create output_dir {}: {e}",
                out_dir.display()
            )));
        }

        let mut exported: Vec<String> = Vec::with_capacity(args.track_indices.len());

        for &idx in &args.track_indices {
            let track = &state.tracks[idx];
            let safe_name = sanitize_name(&track.name);
            let filename = format!("{idx}_{safe_name}.wav");
            let out_path = out_dir.join(&filename);

            // Build a single-track state: one track, unmuted, unsoloed.
            let mut single_state = state.clone();
            single_state.tracks = {
                let mut t = track.clone();
                t.muted = false;
                t.soloed = false;
                vec![t]
            };

            match audio_engine::render_state_to_wav(&single_state, &out_path, None) {
                Ok(_report) => {
                    exported.push(out_path.to_string_lossy().into_owned());
                }
                Err(e) => {
                    return Ok(ToolResult::Error(format!(
                        "render failed for track {idx}: {e}"
                    )));
                }
            }
        }

        let count = exported.len();
        Ok(ToolResult::Ok(json!({
            "exported": exported,
            "count": count,
            "summary": format!(
                "Exported {count} track{} to {}",
                if count == 1 { "" } else { "s" },
                out_dir.display()
            )
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::ExportMultipleTool;
    use crate::Tool;

    #[test]
    fn tool_name() {
        assert_eq!(ExportMultipleTool.name(), "export_multiple");
    }
}
