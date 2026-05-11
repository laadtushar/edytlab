//! Place a named marker or region annotation at the current head.

use serde::Deserialize;
use serde_json::{json, Value};
use session::{Annotation, AnnotationId, AnnotationKind};

use crate::schema::anthropic_tool;
use crate::tool::util::{append_state, load_head_state};
use crate::{Tool, ToolContext, ToolResult};

pub struct LabelTool;

impl Tool for LabelTool {
    fn name(&self) -> &'static str {
        "label"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "label",
            "Place a named marker or region label in the session at the current head. \
             Provide `time` for a point marker or `start`+`end` for a region. \
             Appends a new session node.",
            json!({
                "type": "object",
                "properties": {
                    "name":  { "type": "string", "description": "Display name for the label" },
                    "time":  { "type": "number", "description": "Marker position in seconds (point marker)" },
                    "start": { "type": "number", "description": "Region start in seconds" },
                    "end":   { "type": "number", "description": "Region end in seconds" }
                },
                "required": ["name"],
                "additionalProperties": false
            }),
        )
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        #[derive(Deserialize)]
        struct Args {
            name: String,
            time: Option<f64>,
            start: Option<f64>,
            end: Option<f64>,
        }

        let parsed: Args = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };

        let kind = match (parsed.time, parsed.start, parsed.end) {
            (Some(t), _, _) => AnnotationKind::Marker { time_sec: t },
            (None, Some(s), Some(e)) => {
                if s >= e {
                    return Ok(ToolResult::Error(format!(
                        "region start ({s:.3}) must be < end ({e:.3})"
                    )));
                }
                AnnotationKind::Region {
                    start_sec: s,
                    end_sec: e,
                }
            }
            _ => {
                return Ok(ToolResult::Error(
                    "provide either `time` (point marker) or both `start` and `end` (region)"
                        .into(),
                ))
            }
        };

        let annotation = Annotation {
            id: AnnotationId::new(),
            name: parsed.name.clone(),
            kind,
        };

        let mut state = match load_head_state(ctx) {
            Ok(s) => s,
            Err(e) => return Ok(ToolResult::Error(e)),
        };
        state.annotations.push(annotation);

        let new_id = match append_state(ctx, state, format!("label '{}'", parsed.name)) {
            Ok(id) => id,
            Err(e) => return Ok(ToolResult::Error(e)),
        };

        Ok(ToolResult::Ok(json!({
            "node_id": new_id.to_hex(),
            "summary": format!("placed label '{}'; new head {}", parsed.name, new_id.to_hex()),
        })))
    }
}
