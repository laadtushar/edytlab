//! `SessionContext` — what the agent loop sees about the user's
//! current focus on the timeline. Built per turn from
//! frontend-pushed selection + store-loaded annotations.

use session::{Annotation, AnnotationKind};
use tools::Range;

#[derive(Debug, Clone, Default)]
pub struct SessionContext {
    pub selection: Option<Range>,
    pub markers: Vec<Annotation>,
}

/// Render the context as a deterministic block to splice into the
/// system prompt. Returns an empty string when context is empty so
/// callers can splice unconditionally.
pub fn render_block(ctx: &SessionContext) -> String {
    if ctx.selection.is_none() && ctx.markers.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    if let Some(sel) = ctx.selection {
        out.push_str("## current_selection\n");
        out.push_str(&format!(
            "start: {:.2}s  end: {:.2}s\n\n",
            sel.start_sec, sel.end_sec
        ));
    }
    if !ctx.markers.is_empty() {
        let mut sorted: Vec<&Annotation> = ctx.markers.iter().collect();
        sorted.sort_by(|a, b| {
            time_of(a)
                .partial_cmp(&time_of(b))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        out.push_str("## markers\n");
        for a in sorted {
            match &a.kind {
                AnnotationKind::Marker { time_sec } => {
                    out.push_str(&format!("- {} @ {:.2}s\n", a.name, time_sec));
                }
                AnnotationKind::Region { start_sec, end_sec } => {
                    out.push_str(&format!(
                        "- {} @ {:.2}s-{:.2}s\n",
                        a.name, start_sec, end_sec
                    ));
                }
            }
        }
    }
    out
}

fn time_of(a: &Annotation) -> f64 {
    match a.kind {
        AnnotationKind::Marker { time_sec } => time_sec,
        AnnotationKind::Region { start_sec, .. } => start_sec,
    }
}
