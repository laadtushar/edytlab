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
        ..Default::default()
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
        ..Default::default()
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
        ..Default::default()
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
        ..Default::default()
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
        ..Default::default()
    };
    // `ai::SessionContext` and `ai::session_context::SessionContext`
    // must be the same type, or a caller importing the short path gets
    // a value `render_block` will not take.
    assert!(render_block(&ctx).contains("current_selection"));
}

// ---------------------------------------------------------------------
// One format for the block and for `list_session` (#618)
// ---------------------------------------------------------------------

mod one_format {
    use ai::session_context::{render_block_within, SessionContext};
    use ai::{BusBrief, TrackBrief};
    use serde_json::{json, Value};
    use session::{
        Annotation, AnnotationId, AnnotationKind, BusGraph, Clip, EffectInstance, SessionNode,
        SessionState, Store, TempoMap, Track, TrackId,
    };
    use tools::{ToolContext, ToolDispatcher, ToolResult};

    const BUSES: usize = 3;

    fn bus_id(b: usize) -> String {
        format!("{b:08x}-5b7e-4d21-9a6c-0e1d2f3a4b5c")
    }

    fn effect(kind: &str, bypassed: bool) -> EffectInstance {
        EffectInstance {
            kind: kind.into(),
            params: json!({}),
            bypassed,
        }
    }

    fn clip(start: u64) -> Clip {
        Clip {
            source_path: format!("/nonexistent/take-{start}.wav").into(),
            start_in_track: start * 48_000,
            source_offset: 0,
            length: 24_000,
            content_hash: None,
            time_stretch_factor: None,
            pitch_shift_semitones: None,
            beat_grid: None,
            volume_envelope: Vec::new(),
            volume_envelope_frames: Default::default(),
        }
    }

    /// A session that uses every part of a line: muted and soloed tracks,
    /// tracks with no audio, one clip and many, panned and gained, chains
    /// with a bypassed effect and one longer than a line lists, a bus with
    /// more sends than a line lists, one with none, and markers and
    /// regions held out of time order.
    fn varied() -> SessionState {
        let tracks = (0..14)
            .map(|i| {
                let sends = [(i % 3 == 1, 0), (i % 2 == 0, 1), (i >= 5, 2)]
                    .into_iter()
                    .filter(|(sends, _)| *sends)
                    .map(|(_, b)| {
                        serde_json::from_value(json!({
                            "bus_id": bus_id(b),
                            "level_db": -6.0 - i as f32,
                        }))
                        .expect("a send")
                    })
                    .collect();
                Track {
                    id: TrackId::new(),
                    name: format!("track \"{i}\""),
                    clips: match i % 4 {
                        0 => (0..12).map(clip).collect(),
                        1 => vec![],
                        2 => vec![clip(i as u64)],
                        _ => vec![clip(0), clip(7)],
                    },
                    gain_db: 3.0 - i as f32,
                    pan: ((i % 5) as f32 - 2.0) / 2.0,
                    muted: i % 3 == 0,
                    soloed: i % 4 == 0,
                    effects: vec![effect("eq", false), effect("limiter", i % 2 == 0)],
                    sends,
                }
            })
            .collect();
        let buses = (0..BUSES)
            .map(|b| {
                let effects: Vec<EffectInstance> = match b {
                    0 => vec![effect("reverb", false)],
                    1 => vec![],
                    _ => (0..20).map(|k| effect("eq", k % 2 == 1)).collect(),
                };
                serde_json::from_value(json!({
                    "id": bus_id(b),
                    "name": format!("Bus {b}"),
                    "effects": effects,
                }))
                .expect("a bus")
            })
            .collect();
        let annotations = (0..25)
            .map(|k| {
                let m = (k * 7) % 25;
                let at = m as f64 * 1.37;
                Annotation {
                    id: AnnotationId::new(),
                    name: format!("cue {m}"),
                    kind: if m % 4 == 3 {
                        AnnotationKind::Region {
                            start_sec: at,
                            end_sec: at + 3.25,
                        }
                    } else {
                        AnnotationKind::Marker { time_sec: at }
                    },
                }
            })
            .collect();
        SessionState {
            tracks,
            bus_routing: BusGraph { buses },
            master_chain: Vec::new(),
            tempo_map: TempoMap::default(),
            key_map: None,
            transcript: None,
            sample_rate: 48_000,
            length_samples: 0,
            annotations,
            sync_lock: false,
        }
    }

    /// The `- ` lines under `## <name>` of a block.
    fn block_lines(block: &str, name: &str) -> Vec<String> {
        let heading = format!("## {name}");
        block
            .lines()
            .skip_while(|l| *l != heading)
            .skip(1)
            .take_while(|l| !l.is_empty())
            .filter(|l| l.starts_with("- "))
            .map(str::to_string)
            .collect()
    }

    /// Every line of every page of `what`, two to a page.
    fn tool_lines(store: &mut Store, what: &str) -> Vec<String> {
        let dispatcher = ToolDispatcher::default_dispatcher();
        let mut engine = audio_engine::Engine::new();
        let mut clipboard = None;
        let (mut lines, mut offset, mut pages) = (Vec::new(), 0, 0);
        loop {
            let mut ctx = ToolContext {
                store: &mut *store,
                engine: &mut engine,
                user_message: "",
                clipboard: &mut clipboard,
                allowed_tools: None,
            };
            let result = dispatcher
                .invoke(
                    "list_session",
                    json!({ "what": what, "offset": offset, "limit": 2 }),
                    &mut ctx,
                )
                .unwrap_or_else(|e| panic!("list_session was refused: {e}"));
            let ToolResult::Ok(page) = result else {
                panic!("list_session failed: {result:?}");
            };
            lines.extend(page["text"].as_str().unwrap().lines().map(str::to_string));
            pages += 1;
            match page.get("next_offset").and_then(Value::as_u64) {
                Some(next) => offset = next as usize,
                None => break,
            }
        }
        assert!(pages > 1, "{what}: one page proves nothing about paging");
        lines
    }

    /// The block is the model's first look at the session and `list_session`
    /// its way to the rest, so a line must read the same in both. They are
    /// written by one function; this holds the two together, with every
    /// part of a line in play, against a rewrite that forks them.
    #[test]
    fn list_session_writes_the_lines_the_session_block_writes() {
        let state = varied();
        let dir = tempfile::tempdir().expect("tempdir");
        let mut store = Store::open(dir.path()).expect("open store");
        // The node as the store's own files hold it, so the test needs no
        // clock of its own.
        let node: SessionNode = serde_json::from_value(json!({
            "id": "0".repeat(64),
            "parent": null,
            "created_at": "2026-01-01T00:00:00Z",
            "label": null,
            "reasoning": null,
            "state": state,
        }))
        .expect("a node");
        store.append(node).expect("append");

        let ctx = SessionContext {
            head: Some("ab12".into()),
            tracks: TrackBrief::from_state(&state),
            buses: BusBrief::from_state(&state),
            markers: state.annotations.clone(),
            ..Default::default()
        };
        let block = render_block_within(&ctx, usize::MAX, "");

        for (what, count) in [("tracks", 14), ("buses", BUSES), ("markers", 25)] {
            let listed = block_lines(&block, what);
            assert_eq!(listed.len(), count, "{what}: {block}");
            assert_eq!(tool_lines(&mut store, what), listed, "{what}");
        }
        // The parts of a line this session exercises are in the block, so the
        // comparison above is not between two plain shapes.
        assert!(block.contains("(muted, soloed)"), "{block}");
        assert!(block.contains(": no audio;"), "{block}");
        assert!(block.contains("12 clips:"), "{block}");
        assert!(block.contains("and 4 more;"), "{block}");
        assert!(block.contains("limiter (bypassed)"), "{block}");
        assert!(block.contains("pan 100% left"), "{block}");
        assert!(block.contains(" @ 4.11s-7.36s"), "{block}");
    }
}
