use ai::session_context::{render_block, SessionContext};
use session::{Annotation, AnnotationId, AnnotationKind};
use tools::Range;

fn ann(name: &str, time: f64) -> Annotation {
    Annotation {
        id: AnnotationId::new(),
        name: name.into(),
        kind: AnnotationKind::Marker { time_sec: time },
    }
}

#[test]
fn block_includes_selection_when_present() {
    let ctx = SessionContext {
        selection: Some(Range {
            start_sec: 1.0,
            end_sec: 2.5,
        }),
        markers: vec![],
    };
    let block = render_block(&ctx);
    assert!(block.contains("current_selection"));
    assert!(block.contains("1.00"));
    assert!(block.contains("2.50"));
}

#[test]
fn block_includes_markers_sorted_by_time() {
    let ctx = SessionContext {
        selection: None,
        markers: vec![ann("drop", 78.5), ann("chorus", 42.0)],
    };
    let block = render_block(&ctx);
    let chorus_pos = block.find("chorus").unwrap();
    let drop_pos = block.find("drop").unwrap();
    assert!(
        chorus_pos < drop_pos,
        "markers should be sorted by time ascending"
    );
}

#[test]
fn empty_context_renders_empty_string() {
    let ctx = SessionContext {
        selection: None,
        markers: vec![],
    };
    assert_eq!(render_block(&ctx), "");
}

#[test]
fn region_annotations_render_with_range() {
    let ctx = SessionContext {
        selection: None,
        markers: vec![Annotation {
            id: AnnotationId::new(),
            name: "verse".into(),
            kind: AnnotationKind::Region {
                start_sec: 1.0,
                end_sec: 5.0,
            },
        }],
    };
    let block = render_block(&ctx);
    assert!(block.contains("verse"));
    assert!(block.contains("1.00"));
    assert!(block.contains("5.00"));
}

#[test]
fn agent_loop_accepts_session_context() {
    // Compile-test: ensure the API surface exists.
    fn _accepts(_: ai::SessionContext) {}
}
