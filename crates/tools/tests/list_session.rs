//! `list_session`: the model can list every track, bus and marker of a
//! session, a page at a time (#618).
//!
//! When a session is too big for the system prompt's session block, the
//! block cuts its track, bus and marker lists and says how many it left
//! out (#573). No tool listed the tracks or the buses, and
//! `export_labels` returned every marker in one result (about 25 KB for a
//! thousand), too big for an 8,192-token context. `list_session` is the
//! way to the rest: one read-only call that returns one page of one list,
//! in the lines the block itself uses, capped by bytes as well as by
//! count.
//!
//! These tests go through the dispatcher, as the model does. The session
//! is a real store with a real head node.

use serde_json::{json, Value};
use session::{
    Annotation, AnnotationId, AnnotationKind, BusGraph, Clip, EffectInstance, NodeId, SessionNode,
    SessionState, Store, TempoMap, Track, TrackId,
};
use tempfile::TempDir;
use tools::{DispatchError, ToolContext, ToolDispatcher, ToolResult};

/// The most a page may take, in bytes of the result as the model receives
/// it (the JSON of the whole object). The tool states the same number in
/// `tool::list_session::PAGE_MAX_BYTES`; the assertion in
/// `the_cap_is_the_one_the_tests_state` keeps the two together.
const STATED_PAGE_CAP: usize = 4_096;

struct Fixture {
    _dir: TempDir,
    store: Store,
    engine: audio_engine::Engine,
    clipboard: Option<tools::Clipboard>,
    dispatcher: ToolDispatcher,
}

impl Fixture {
    /// A store with no head: nothing loaded.
    fn empty() -> Self {
        let dir = TempDir::new().expect("tempdir");
        let store = Store::open(dir.path()).expect("open store");
        Self {
            _dir: dir,
            store,
            engine: audio_engine::Engine::new(),
            clipboard: None,
            dispatcher: ToolDispatcher::default_dispatcher(),
        }
    }

    /// A store whose head holds `state`.
    fn with(state: SessionState) -> Self {
        let mut f = Self::empty();
        f.set_head(state);
        f
    }

    fn set_head(&mut self, state: SessionState) {
        self.store
            .append(SessionNode {
                id: NodeId([0u8; 32]),
                parent: None,
                created_at: chrono::Utc::now(),
                label: Some("fixture".into()),
                reasoning: None,
                state,
                op: None,
            })
            .expect("append the head");
    }

    fn try_call(&mut self, args: Value) -> Result<ToolResult, DispatchError> {
        let mut ctx = ToolContext {
            store: &mut self.store,
            engine: &mut self.engine,
            user_message: "",
            clipboard: &mut self.clipboard,
            allowed_tools: None,
        };
        self.dispatcher.invoke("list_session", args, &mut ctx)
    }

    fn call(&mut self, args: Value) -> ToolResult {
        self.try_call(args)
            .unwrap_or_else(|e| panic!("list_session was refused: {e}"))
    }

    fn page(&mut self, args: Value) -> Page {
        match self.call(args.clone()) {
            ToolResult::Ok(value) => Page::of(&value),
            ToolResult::Error(e) => panic!("list_session {args} failed: {e}"),
        }
    }

    /// Every page of `what`, from the first, following `next_offset` the
    /// way the model is told to. `limit` is passed when given.
    fn pages(&mut self, what: &str, limit: Option<usize>) -> Vec<Page> {
        let mut pages = Vec::new();
        let mut offset = 0;
        loop {
            let mut args = json!({ "what": what, "offset": offset });
            if let Some(limit) = limit {
                args["limit"] = json!(limit);
            }
            let page = self.page(args);
            let next = page.next_offset;
            pages.push(page);
            assert!(pages.len() < 1_000, "the pages of {what} never end");
            match next {
                Some(next) => {
                    assert!(next > offset, "{what}: next_offset {next} does not advance");
                    offset = next;
                }
                None => return pages,
            }
        }
    }
}

/// One result, taken apart.
struct Page {
    summary: String,
    text: String,
    next_offset: Option<usize>,
    /// The result as the model receives it.
    bytes: usize,
}

impl Page {
    fn of(value: &Value) -> Self {
        Self {
            summary: value["summary"]
                .as_str()
                .unwrap_or_else(|| panic!("no summary: {value}"))
                .to_string(),
            text: value["text"]
                .as_str()
                .unwrap_or_else(|| panic!("no text: {value}"))
                .to_string(),
            next_offset: value.get("next_offset").map(|n| {
                n.as_u64()
                    .unwrap_or_else(|| panic!("next_offset is not a number: {value}"))
                    as usize
            }),
            bytes: serde_json::to_string(value).expect("json").len(),
        }
    }

    fn lines(&self) -> Vec<&str> {
        self.text.lines().collect()
    }
}

/// Every line of every page, in order.
fn lines_of(pages: &[Page]) -> Vec<String> {
    pages
        .iter()
        .flat_map(|p| p.lines().into_iter().map(str::to_string))
        .collect()
}

fn bus_id(b: usize) -> String {
    format!("{b:08x}-5b7e-4d21-9a6c-0e1d2f3a4b5c")
}

fn effect(kind: &str) -> EffectInstance {
    EffectInstance {
        kind: kind.into(),
        params: json!({}),
        bypassed: false,
    }
}

/// The track `i`, named `name`, with one clip 184.5 s long, a compressor
/// and an EQ, and a send to bus `i % buses` when there are buses.
fn track(i: usize, name: String, buses: usize) -> Track {
    let sends = if buses == 0 {
        Vec::new()
    } else {
        vec![serde_json::from_value(json!({
            "bus_id": bus_id(i % buses),
            "level_db": -6.0,
        }))
        .expect("a send")]
    };
    Track {
        id: TrackId::new(),
        name,
        clips: vec![Clip {
            // Not on disk: a clip whose header cannot be read is counted
            // in the session's rate, which is 48 kHz here.
            source_path: format!("/nonexistent/take-{i}.wav").into(),
            start_in_track: 0,
            source_offset: 0,
            length: 8_856_000,
            content_hash: None,
            time_stretch_factor: None,
            pitch_shift_semitones: None,
            beat_grid: None,
            volume_envelope: Vec::new(),
        }],
        gain_db: -2.5,
        pan: 0.0,
        muted: false,
        soloed: false,
        effects: vec![effect("compressor"), effect("eq")],
        sends,
    }
}

fn bus(b: usize, name: String) -> session::Bus {
    serde_json::from_value(json!({ "id": bus_id(b), "name": name, "effects": [] })).expect("a bus")
}

/// Every fifth marker is a region. Markers `m` sit at `m * 0.18` seconds,
/// and are stored out of time order (stepping by 7 through a thousand
/// visits each once), so listing them in time order is not the order they
/// are held in.
fn markers(n: usize, name: impl Fn(usize) -> String) -> Vec<Annotation> {
    (0..n)
        .map(|k| {
            let m = (k * 7) % n;
            let start = m as f64 * 0.18;
            Annotation {
                id: AnnotationId::new(),
                name: name(m),
                kind: if m % 5 == 4 {
                    AnnotationKind::Region {
                        start_sec: start,
                        end_sec: start + 2.0,
                    }
                } else {
                    AnnotationKind::Marker { time_sec: start }
                },
            }
        })
        .collect()
}

fn state(
    tracks: Vec<Track>,
    buses: Vec<session::Bus>,
    annotations: Vec<Annotation>,
) -> SessionState {
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

/// Two hundred tracks, fifty buses and a thousand markers: the session
/// whose block is cut on every provider.
fn huge() -> SessionState {
    state(
        (0..200)
            .map(|i| track(i, format!("strings {i}"), 50))
            .collect(),
        (0..50).map(|b| bus(b, format!("Bus {b}"))).collect(),
        markers(1_000, |m| format!("cue {m}")),
    )
}

// ---------------------------------------------------------------------
// The acceptance: every track, bus and marker, page by page
// ---------------------------------------------------------------------

/// The tracks come out in index order, one line each, and the lines are
/// the session block's own.
#[test]
fn every_track_is_listed_page_by_page() {
    let mut f = Fixture::with(huge());

    let pages = f.pages("tracks", None);

    assert!(pages.len() > 1, "200 tracks fit one page");
    let lines = lines_of(&pages);
    assert_eq!(lines.len(), 200, "{} lines", lines.len());
    for (i, line) in lines.iter().enumerate() {
        let expected = format!(
            "- {i} \"strings {i}\": 0.00-184.50s; gain -2.5 dB; pan centre; effects: compressor, eq"
        );
        assert_eq!(line, &expected);
    }
    for page in &pages {
        assert!(page.summary.contains("of 200 tracks"), "{}", page.summary);
    }
    // The last page ends the list, and says so.
    let last = pages.last().unwrap();
    assert_eq!(last.next_offset, None);
    assert!(last.summary.contains("last page"), "{}", last.summary);
}

/// Each bus with its id, which tracks feed it and its chain.
#[test]
fn every_bus_is_listed_page_by_page() {
    let mut f = Fixture::with(huge());

    let pages = f.pages("buses", None);

    let lines = lines_of(&pages);
    assert_eq!(lines.len(), 50, "{lines:#?}");
    for (b, line) in lines.iter().enumerate() {
        // Bus b is fed by tracks b, b + 50, b + 100 and b + 150.
        let expected = format!(
            "- \"Bus {b}\" (id {id}): from {b} \"strings {b}\" (-6.0 dB), {t1} \"strings {t1}\" (-6.0 dB), {t2} \"strings {t2}\" (-6.0 dB), {t3} \"strings {t3}\" (-6.0 dB)",
            id = bus_id(b),
            t1 = b + 50,
            t2 = b + 100,
            t3 = b + 150,
        );
        assert_eq!(line, &expected);
    }
}

/// Markers and regions in time order, whatever order they are held in.
#[test]
fn every_marker_is_listed_page_by_page_in_time_order() {
    let mut f = Fixture::with(huge());

    let pages = f.pages("markers", None);

    assert!(pages.len() > 1, "1,000 markers fit one page");
    let lines = lines_of(&pages);
    assert_eq!(lines.len(), 1_000, "{} lines", lines.len());
    for (m, line) in lines.iter().enumerate() {
        let start = m as f64 * 0.18;
        let expected = if m % 5 == 4 {
            format!("- cue {m} @ {start:.2}s-{:.2}s", start + 2.0)
        } else {
            format!("- cue {m} @ {start:.2}s")
        };
        assert_eq!(line, &expected);
    }
}

/// `limit` is the most items a page holds, and the next page starts where
/// this one stopped.
#[test]
fn limit_sets_how_many_a_page_holds() {
    let mut f = Fixture::with(huge());

    let first = f.page(json!({ "what": "markers", "limit": 7 }));
    assert_eq!(first.lines().len(), 7, "{}", first.text);
    assert_eq!(first.next_offset, Some(7));
    assert!(
        first.lines()[0].starts_with("- cue 0 @ 0.00s"),
        "{}",
        first.text
    );

    let second = f.page(json!({ "what": "markers", "offset": 7, "limit": 7 }));
    assert!(
        second.lines()[0].starts_with("- cue 7 @ 1.26s"),
        "{}",
        second.text
    );
}

// ---------------------------------------------------------------------
// The byte cap
// ---------------------------------------------------------------------

#[test]
fn the_cap_is_the_one_the_tests_state() {
    assert_eq!(tools::tool::list_session::PAGE_MAX_BYTES, STATED_PAGE_CAP);
}

/// No page of any list passes the cap, as the model receives it, and the
/// pages still reach every item.
#[test]
fn no_page_passes_the_byte_cap() {
    let mut f = Fixture::with(huge());
    for (what, total) in [("tracks", 200), ("buses", 50), ("markers", 1_000)] {
        // Without a limit, and with the largest the tool takes.
        for limit in [None, Some(10_000)] {
            let pages = f.pages(what, limit);
            for page in &pages {
                assert!(
                    page.bytes <= STATED_PAGE_CAP,
                    "a page of {what} is {} B, over {STATED_PAGE_CAP} B",
                    page.bytes
                );
            }
            assert_eq!(lines_of(&pages).len(), total, "{what}");
        }
    }
}

/// A page stops early, at the cap, not at the count: names this long make
/// the page hold far fewer than it is allowed to.
#[test]
fn a_page_stops_at_the_bytes_when_the_lines_are_long() {
    let long = "x".repeat(300);
    let mut f = Fixture::with(state(
        (0..60)
            .map(|i| track(i, format!("{long} {i}"), 0))
            .collect(),
        vec![],
        vec![],
    ));

    let pages = f.pages("tracks", None);

    let first = &pages[0];
    assert!(first.lines().len() < 20, "{} lines", first.lines().len());
    assert!(first.next_offset.is_some());
    assert!(first.bytes <= STATED_PAGE_CAP, "{} B", first.bytes);
    assert_eq!(lines_of(&pages).len(), 60);
}

/// One line longer than the cap cannot be listed whole, and must not stall
/// the list or pass the cap: it is cut, marked, and the next page goes on.
#[test]
fn a_line_longer_than_the_cap_is_cut_and_the_list_goes_on() {
    let mut names: Vec<String> = (0..10).map(|m| format!("cue {m}")).collect();
    names[0] = format!("cue 0 {}", "y".repeat(10_000));
    let marks = names
        .iter()
        .enumerate()
        .map(|(m, name)| Annotation {
            id: AnnotationId::new(),
            name: name.clone(),
            kind: AnnotationKind::Marker { time_sec: m as f64 },
        })
        .collect();
    let mut f = Fixture::with(state(vec![], vec![], marks));

    let pages = f.pages("markers", None);

    let first = &pages[0];
    assert!(first.bytes <= STATED_PAGE_CAP, "{} B", first.bytes);
    assert!(
        first.text.starts_with("- cue 0 yyy"),
        "{}",
        &first.text[..40]
    );
    assert!(first.text.trim_end().ends_with('…'), "the cut is marked");
    // The rest follows: nine more markers, whichever page they fall on.
    let rest: Vec<String> = lines_of(&pages[1..]);
    let on_first = first.lines().len() - 1;
    assert_eq!(on_first + rest.len(), 9, "{rest:?}");
}

// ---------------------------------------------------------------------
// What a page says about itself
// ---------------------------------------------------------------------

#[test]
fn a_page_says_what_range_it_showed_of_how_many_and_where_to_go_on() {
    let mut f = Fixture::with(huge());

    let page = f.page(json!({ "what": "buses", "limit": 20 }));
    assert_eq!(page.next_offset, Some(20));
    assert!(
        page.summary.contains("20 of 50 buses") && page.summary.contains("offsets 0-19"),
        "{}",
        page.summary
    );
    assert!(page.summary.contains("offset 20"), "{}", page.summary);

    let page = f.page(json!({ "what": "buses", "offset": 40, "limit": 20 }));
    assert_eq!(page.next_offset, None);
    assert!(
        page.summary.contains("10 of 50 buses") && page.summary.contains("offsets 40-49"),
        "{}",
        page.summary
    );
}

#[test]
fn an_offset_past_the_end_and_an_empty_list_say_so() {
    let mut f = Fixture::with(huge());
    let past = f.page(json!({ "what": "tracks", "offset": 500 }));
    assert_eq!(past.text, "");
    assert_eq!(past.next_offset, None);
    assert!(
        past.summary.contains("past the end") && past.summary.contains("200"),
        "{}",
        past.summary
    );

    let mut none = Fixture::with(state(vec![track(0, "voice".into(), 0)], vec![], vec![]));
    for what in ["buses", "markers"] {
        let page = none.page(json!({ "what": what }));
        assert_eq!(page.text, "");
        assert_eq!(page.next_offset, None);
        assert!(
            page.summary.contains(&format!("no {what}")),
            "{}",
            page.summary
        );
    }
}

/// The list is the head's, read at the call: an edit between two calls
/// shows in the second.
#[test]
fn the_list_is_the_heads_at_the_time_of_the_call() {
    let mut f = Fixture::with(state(vec![track(0, "voice".into(), 0)], vec![], vec![]));
    assert_eq!(f.page(json!({ "what": "tracks" })).lines().len(), 1);

    f.set_head(state(
        vec![track(0, "voice".into(), 0), track(1, "music".into(), 0)],
        vec![],
        vec![],
    ));

    let page = f.page(json!({ "what": "tracks" }));
    assert_eq!(page.lines().len(), 2, "{}", page.text);
    assert!(
        page.lines()[1].starts_with("- 1 \"music\""),
        "{}",
        page.text
    );
}

// ---------------------------------------------------------------------
// Arguments
// ---------------------------------------------------------------------

/// A `limit` over the most a page holds is a smaller page, not a refused
/// call: a second refusal in a row ends the turn.
#[test]
fn a_limit_over_the_maximum_is_clamped_not_refused() {
    let mut f = Fixture::with(huge());
    let page = f.page(json!({ "what": "markers", "limit": 1_000_000 }));
    assert!(page.lines().len() > 1);
    assert!(page.next_offset.is_some());
}

#[test]
fn what_must_be_one_of_the_three_lists() {
    let mut f = Fixture::with(huge());
    for args in [
        json!({}),
        json!({ "what": "clips" }),
        json!({ "what": "tracks", "offset": -1 }),
        json!({ "what": "tracks", "limit": 0 }),
    ] {
        assert!(
            matches!(
                f.try_call(args.clone()),
                Err(DispatchError::SchemaValidation(_))
            ),
            "{args} was accepted"
        );
    }
}

#[test]
fn with_no_session_loaded_it_says_so() {
    let mut f = Fixture::empty();
    match f.call(json!({ "what": "tracks" })) {
        ToolResult::Error(e) => assert!(e.contains("no session loaded"), "{e}"),
        other => panic!("{other:?}"),
    }
}

// ---------------------------------------------------------------------
// Read-only, and never held
// ---------------------------------------------------------------------

/// It changes nothing, so Plan first never holds it, and it is not on the
/// list of tools that run off the lock.
#[test]
fn it_is_registered_read_only_and_runs_under_the_lock() {
    let dispatcher = ToolDispatcher::default_dispatcher();
    let tool = dispatcher
        .get("list_session")
        .expect("list_session is a registered tool");
    assert!(!tool.mutates());
    assert!(!tool.runs_off_the_lock());
    assert!(!dispatcher.would_mutate("list_session", &json!({ "what": "tracks" }), None));
}
