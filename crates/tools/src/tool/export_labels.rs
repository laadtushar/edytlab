use serde::Deserialize;
use serde_json::{json, Value};

use crate::schema::anthropic_tool;
use crate::tool::util::load_head_state;
use crate::{Tool, ToolContext, ToolResult};
use session::annotation::{Annotation, AnnotationKind};

pub(crate) fn format_labels(annotations: &[Annotation]) -> String {
    annotations
        .iter()
        .map(|a| match &a.kind {
            AnnotationKind::Marker { time_sec } => {
                format!("{time_sec}\t{time_sec}\t{}\n", a.name)
            }
            AnnotationKind::Region { start_sec, end_sec } => {
                format!("{start_sec}\t{end_sec}\t{}\n", a.name)
            }
        })
        .collect()
}

#[derive(Debug, Deserialize)]
struct Args {}

pub struct ExportLabelsTool;

impl Tool for ExportLabelsTool {
    fn name(&self) -> &'static str {
        "export_labels"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "export_labels",
            "Export session annotations as Audacity-format label text (start_sec TAB end_sec TAB name, one per line). Does not modify audio. Returns the label text.",
            serde_json::json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        )
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let _args: Args = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };
        let state = match load_head_state(ctx) {
            Ok(s) => s,
            Err(e) => return Ok(ToolResult::Error(e)),
        };
        let count = state.annotations.len();
        let label_text = format_labels(&state.annotations);
        Ok(ToolResult::Ok(json!({
            "labels": label_text,
            "count": count,
            "summary": format!("Exported {count} label(s) in Audacity format")
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::format_labels;
    use session::annotation::{Annotation, AnnotationId, AnnotationKind};
    use uuid::Uuid;

    fn make_region(start: f64, end: f64, name: &str) -> Annotation {
        Annotation {
            id: AnnotationId(Uuid::nil()),
            name: name.to_string(),
            kind: AnnotationKind::Region {
                start_sec: start,
                end_sec: end,
            },
        }
    }

    fn make_marker(time: f64, name: &str) -> Annotation {
        Annotation {
            id: AnnotationId(Uuid::nil()),
            name: name.to_string(),
            kind: AnnotationKind::Marker { time_sec: time },
        }
    }

    #[test]
    fn formats_region_correctly() {
        let ann = make_region(1.5, 3.0, "verse");
        let line = format_labels(&[ann]);
        assert!(
            line.contains("1.5") && line.contains("verse"),
            "format: {line:?}"
        );
    }

    #[test]
    fn formats_marker_as_point() {
        let ann = make_marker(2.5, "cue");
        let line = format_labels(&[ann]);
        assert!(
            line.contains("2.5") && line.contains("cue"),
            "marker format: {line:?}"
        );
    }

    #[test]
    fn empty_gives_empty_string() {
        assert_eq!(format_labels(&[]), "");
    }
}
