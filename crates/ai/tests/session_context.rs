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

/// This pins the crate-root re-export, and nothing more.
///
/// It was called `agent_loop_accepts_session_context` and its whole
/// body was `fn _accepts(_: ai::SessionContext) {}` — a function that
/// was never called, in a file that never names `agent_loop`. It would
/// have passed with the context dropped from the system prompt
/// outright, which is the opposite of what the name promised.
///
/// The wiring itself is covered where it can actually be reached:
/// `a_session_context_reaches_the_prompt_in_the_context_slot` in
/// `agent_loop.rs`, which runs a real context through `render_block`
/// into the assembler and checks which slot it lands in.
#[test]
fn session_context_is_re_exported_at_the_crate_root() {
    let ctx = ai::SessionContext {
        selection: Some(Range {
            start_sec: 0.0,
            end_sec: 1.0,
        }),
        markers: vec![],
    };
    // `ai::SessionContext` and `ai::session_context::SessionContext`
    // must be the same type, or a caller importing the short path gets
    // a value `render_block` will not take.
    assert!(render_block(&ctx).contains("current_selection"));
}
