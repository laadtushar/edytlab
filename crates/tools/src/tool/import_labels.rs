use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::schema::anthropic_tool;
use crate::tool::util::{append_state, load_head_state};
use crate::{Tool, ToolContext, ToolResult};
use session::annotation::{Annotation, AnnotationId, AnnotationKind};

pub(crate) fn parse_labels(text: &str) -> Vec<(f64, f64, String)> {
    text.lines()
        .filter_map(|line| {
            let parts: Vec<&str> = line.splitn(3, '\t').collect();
            if parts.len() < 3 {
                return None;
            }
            let start: f64 = parts[0].trim().parse().ok()?;
            let end: f64 = parts[1].trim().parse().ok()?;
            let label = parts[2].trim().to_string();
            Some((start, end, label))
        })
        .collect()
}

#[derive(Debug, Deserialize)]
struct Args {
    labels_text: String,
}

pub struct ImportLabelsTool;

impl Tool for ImportLabelsTool {
    fn name(&self) -> &'static str {
        "import_labels"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "import_labels",
            "Import Audacity-format label text into the session as annotations. Format: each line is 'start_sec TAB end_sec TAB name'. Appends a new session node.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "labels_text": { "type": "string", "description": "Label file content in Audacity format" }
                },
                "required": ["labels_text"]
            }),
        )
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };
        let parsed = parse_labels(&args.labels_text);
        if parsed.is_empty() {
            return Ok(ToolResult::Error(
                "No valid labels found. Expected format: 'start_sec TAB end_sec TAB name' per line.".into(),
            ));
        }
        let mut state = match load_head_state(ctx) {
            Ok(s) => s,
            Err(e) => return Ok(ToolResult::Error(e)),
        };
        let new_annotations: Vec<Annotation> = parsed
            .iter()
            .map(|(start, end, name)| {
                let kind = if (start - end).abs() < 1e-9 {
                    AnnotationKind::Marker { time_sec: *start }
                } else {
                    AnnotationKind::Region {
                        start_sec: *start,
                        end_sec: *end,
                    }
                };
                Annotation {
                    id: AnnotationId(Uuid::new_v4()),
                    name: name.clone(),
                    kind,
                }
            })
            .collect();
        let count = new_annotations.len();
        state.annotations.extend(new_annotations);
        let new_id = match append_state(ctx, state, format!("import_labels {count} label(s)")) {
            Ok(id) => id,
            Err(e) => return Ok(ToolResult::Error(e)),
        };
        Ok(ToolResult::Ok(json!({
            "node_id": new_id.to_hex(),
            "imported": count,
            "summary": format!("Imported {count} label(s)")
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::parse_labels;

    #[test]
    fn parses_two_lines() {
        let text = "1.5\t3.0\tverse\n4.0\t6.5\tchorus\n";
        let labels = parse_labels(text);
        assert_eq!(labels.len(), 2);
        assert_eq!(labels[0].0, 1.5);
        assert_eq!(labels[0].1, 3.0);
        assert_eq!(labels[0].2, "verse");
    }

    #[test]
    fn skips_malformed_lines() {
        let text = "bad_line\n1.0\t2.0\tok\n";
        let labels = parse_labels(text);
        assert_eq!(labels.len(), 1);
        assert_eq!(labels[0].2, "ok");
    }
}
