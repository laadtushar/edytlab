//! Hearing an effect on the master bus or on a bus before it becomes a
//! node (#533).
//!
//! `audition_effect` took a track only, so the agent could let the user
//! hear an effect on one track and nowhere else: "a limiter on the master"
//! (#460) had to be added, listened to and undone. It takes the same
//! `scope` as the five chain tools now, and a bus the way they do.
//!
//! The claims are the ones `audition.rs` makes for a track, for the other
//! two chains:
//!
//! * **nothing is committed**: same head, same node count;
//! * it is **the result, minus the commit**: the clip is, sample for
//!   sample, what the session renders once `add_effect` with the same
//!   arguments has been called, which is also different from the session
//!   as it was;
//! * a kind the renderer cannot run is **refused** as it is for a track,
//!   with the kinds that work, and nothing is rendered or cached;
//! * a call that does not name one chain is refused with what to pass, and
//!   a track audition means what it always meant.

use std::path::{Path, PathBuf};

use hound::{SampleFormat, WavReader, WavSpec, WavWriter};
use serde_json::{json, Value};
use tempfile::TempDir;
use tools::{DispatchError, ToolContext, ToolDispatcher, ToolResult};

const SAMPLE_RATE: u32 = 44_100;
const FRAMES: usize = 44_100;

const STREAMABLE: [&str; 5] = [
    "gain",
    "limiter",
    "low_pass_filter",
    "high_pass_filter",
    "notch_filter",
];

const NOT_YET_STREAMABLE: [&str; 11] = [
    "tremolo",
    "phaser",
    "echo",
    "reverb",
    "noise_gate",
    "leveler",
    "de_esser",
    "distortion",
    "stereo_widener",
    "eq",
    "compressor",
];

fn write_tone(dir: &Path, name: &str, freq: f32, amp: f32) -> PathBuf {
    let path = dir.join(name);
    let spec = WavSpec {
        channels: 1,
        sample_rate: SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut w = WavWriter::create(&path, spec).expect("wav writer");
    for n in 0..FRAMES {
        let t = n as f32 / SAMPLE_RATE as f32;
        let s = (2.0 * std::f32::consts::PI * freq * t).sin() * amp;
        w.write_sample((s * 32_767.0) as i16).unwrap();
    }
    w.finalize().unwrap();
    path
}

/// A two-track session at 0.6 each. Their peaks line up every 1/110 s, so
/// the sum reaches about 1.2: a limiter on the mix has something to do.
struct Session {
    dir: TempDir,
    store: session::Store,
    engine: audio_engine::Engine,
    dispatcher: ToolDispatcher,
    clipboard: Option<tools::Clipboard>,
    renders: usize,
}

impl Session {
    fn new() -> Self {
        let dir = TempDir::new().expect("tempdir");
        let a = write_tone(dir.path(), "a.wav", 220.0, 0.6);
        let b = write_tone(dir.path(), "b.wav", 330.0, 0.6);
        let store = session::Store::open(dir.path()).expect("open store");
        let mut s = Self {
            dir,
            store,
            engine: audio_engine::Engine::new(),
            dispatcher: ToolDispatcher::default_dispatcher(),
            clipboard: None,
            renders: 0,
        };
        ok(s.call("load", json!({ "path": a.to_string_lossy() })));
        ok(s.call("load", json!({ "path": b.to_string_lossy() })));
        s
    }

    fn try_call(&mut self, tool: &str, args: Value) -> Result<ToolResult, DispatchError> {
        let mut ctx = ToolContext {
            store: &mut self.store,
            engine: &mut self.engine,
            user_message: "",
            clipboard: &mut self.clipboard,
            allowed_tools: None,
        };
        self.dispatcher.invoke(tool, args, &mut ctx)
    }

    fn call(&mut self, tool: &str, args: Value) -> ToolResult {
        self.try_call(tool, args)
            .unwrap_or_else(|e| panic!("{tool} was rejected before it ran: {e}"))
    }

    fn head(&self) -> String {
        self.store.head().expect("a head").to_hex()
    }

    fn node_count(&self) -> usize {
        self.store.list_nodes().expect("list nodes").len()
    }

    /// How many clips the audition cache holds.
    fn cached_auditions(&self) -> usize {
        let cache = self.dir.path().join(".audiograph").join("auditions");
        std::fs::read_dir(cache).map(|d| d.count()).unwrap_or(0)
    }

    /// The head rendered over the first second, which is all of it, the
    /// way an audition renders its region.
    fn render(&mut self) -> Vec<i16> {
        self.renders += 1;
        let out = self.dir.path().join(format!("out-{}.wav", self.renders));
        let state = self.store.get(self.store.head().unwrap()).unwrap().state;
        let range = audio_engine::TimeRange {
            start_frame: 0,
            end_frame: FRAMES as u64,
        };
        audio_engine::render_state_to_wav(&state, &out, Some(range)).expect("render");
        read_wav(&out)
    }

    fn create_bus(&mut self, name: &str) -> String {
        let v = ok(self.call("create_bus", json!({ "name": name })));
        v["bus_id"].as_str().expect("a bus id").to_string()
    }

    fn audition(&mut self, args: Value) -> ToolResult {
        self.call("audition_effect", args)
    }
}

fn read_wav(path: &Path) -> Vec<i16> {
    WavReader::open(path)
        .expect("open wav")
        .samples::<i16>()
        .map(|r| r.expect("sample"))
        .collect()
}

fn ok(r: ToolResult) -> Value {
    match r {
        ToolResult::Ok(v) => v,
        ToolResult::Error(m) => panic!("expected Ok, got Error({m})"),
    }
}

fn err(r: ToolResult) -> String {
    match r {
        ToolResult::Error(m) => m,
        ToolResult::Ok(v) => panic!("expected Error, got Ok({v})"),
    }
}

fn clip(v: &Value) -> Vec<i16> {
    read_wav(Path::new(v["path"].as_str().expect("a path")))
}

fn peak(pcm: &[i16]) -> f32 {
    pcm.iter().map(|v| (*v as f32).abs()).fold(0.0, f32::max)
}

fn level(db: f32) -> f32 {
    10.0f32.powf(db / 20.0) * 32_767.0
}

// ─── the master bus ───────────────────────────────────────────────────

/// **The ticket.** A master audition changes nothing: same head, same node
/// count, and the result says how to keep it.
#[test]
fn a_master_audition_commits_nothing() {
    let mut s = Session::new();
    let head = s.head();
    let nodes = s.node_count();

    let v = ok(s.audition(json!({
        "scope": "master", "kind": "limiter", "params": { "ceiling_db": -6.0 }
    })));

    assert!(PathBuf::from(v["path"].as_str().unwrap()).is_file());
    assert_eq!(v["type"], "audition", "{v}");
    assert_eq!(s.head(), head, "the head must not move");
    assert_eq!(s.node_count(), nodes, "no node may be appended");
    assert!(
        s.store
            .get(s.store.head().unwrap())
            .unwrap()
            .state
            .master_chain
            .is_empty(),
        "the master chain must not be touched"
    );
    let summary = v["summary"].as_str().unwrap();
    assert!(
        summary.contains("master") && summary.contains("add_effect"),
        "the result should say where and how to keep it: {summary}"
    );
}

/// The point of hearing it first: the clip is what the session sounds like
/// once the effect is there. Compared with the render after `add_effect`
/// with the same arguments, sample by sample, and with the session as it
/// was, which has to be different or the audition heard nothing.
#[test]
fn a_master_audition_is_what_add_effect_would_render() {
    let mut s = Session::new();
    let dry = s.render();
    assert!(
        peak(&dry) > level(-3.0),
        "the premise failed: the dry mix peaks at {} and never needs limiting",
        peak(&dry)
    );

    let args = json!({ "kind": "limiter", "params": { "ceiling_db": -6.0 } });
    let mut audition_args = args.clone();
    audition_args["scope"] = json!("master");
    audition_args["start_sec"] = json!(0.0);
    audition_args["end_sec"] = json!(1.0);
    let heard = clip(&ok(s.audition(audition_args)));

    let mut add_args = args;
    add_args["scope"] = json!("master");
    ok(s.call("add_effect", add_args));
    let result = s.render();

    assert_eq!(
        heard, result,
        "the audition is not what the session renders once the effect is added"
    );
    assert_ne!(heard, dry, "the audition is the session unchanged");
    assert!(
        peak(&heard) <= level(-6.0) + 2.0,
        "peak {} is over the -6 dBFS ceiling: the effect did not reach the clip",
        peak(&heard)
    );
}

/// The effect goes into the chain the way `add_effect` puts it: at the end
/// unless `at` says otherwise, after what the master already holds. Both
/// placements are compared with `add_effect`'s own, and are different
/// sounds.
#[test]
fn a_master_audition_goes_into_the_existing_chain_where_asked() {
    let mut s = Session::new();
    ok(s.call(
        "add_effect",
        json!({ "scope": "master", "kind": "limiter", "params": { "ceiling_db": -6.0 } }),
    ));
    let base = s.store.head().unwrap();
    let nodes = s.node_count();
    let whole = json!({ "start_sec": 0.0, "end_sec": 1.0 });

    let mut at_end = json!({ "scope": "master", "kind": "gain", "params": { "db": 6.0 } });
    let mut at_front = at_end.clone();
    at_front["at"] = json!(0);
    for a in [&mut at_end, &mut at_front] {
        a["start_sec"] = whole["start_sec"].clone();
        a["end_sec"] = whole["end_sec"].clone();
    }
    let heard_end = clip(&ok(s.audition(at_end)));
    let heard_front = clip(&ok(s.audition(at_front)));
    assert_eq!(s.node_count(), nodes);
    assert_eq!(s.store.head().unwrap(), base);

    ok(s.call(
        "add_effect",
        json!({ "scope": "master", "kind": "gain", "params": { "db": 6.0 } }),
    ));
    assert_eq!(heard_end, s.render(), "the default is the end of the chain");
    s.store.set_head(base).unwrap();
    ok(s.call(
        "add_effect",
        json!({
            "scope": "master", "kind": "gain", "params": { "db": 6.0 }, "position": 0
        }),
    ));
    assert_eq!(
        heard_front,
        s.render(),
        "`at` is where add_effect's `position` is"
    );

    assert_ne!(heard_end, heard_front, "the two placements sound the same");
    assert!(
        peak(&heard_front) <= level(-6.0) + 2.0,
        "a gain before the limiter stays limited: {}",
        peak(&heard_front)
    );
    assert!(
        peak(&heard_end) > level(-6.0) + 1_000.0,
        "a gain after the limiter pushes the mix back over its ceiling: {}",
        peak(&heard_end)
    );
}

/// A kind the renderer cannot run would audition as the session unchanged.
/// It is refused for the master as it is for a track, with the kinds that
/// work, and nothing is rendered or cached.
#[test]
fn a_master_audition_refuses_each_kind_the_renderer_cannot_run() {
    let mut s = Session::new();
    let head = s.head();
    let nodes = s.node_count();

    for kind in NOT_YET_STREAMABLE {
        let msg = err(s.audition(json!({ "scope": "master", "kind": kind })));
        assert!(msg.contains(kind), "{kind}: should name the kind: {msg}");
        for works in STREAMABLE {
            assert!(
                msg.contains(works),
                "{kind}: should list `{works}` among the kinds that work: {msg}"
            );
        }
        assert!(
            msg.contains(&format!("`{kind}` tool")),
            "{kind}: should point at the destructive tool of that name: {msg}"
        );
    }
    for kind in ["wobbulator", "Reverb", "GAIN", " gain", ""] {
        let msg = err(s.audition(json!({ "scope": "master", "kind": kind })));
        assert!(msg.contains("unknown effect kind"), "{kind:?}: {msg}");
    }

    assert_eq!(s.head(), head);
    assert_eq!(s.node_count(), nodes);
    assert_eq!(
        s.cached_auditions(),
        0,
        "a refused audition must render nothing"
    );
}

/// The same kind refusal comes first, whatever else is wrong with the call.
#[test]
fn the_kind_is_refused_before_the_bus_is_looked_up() {
    let mut s = Session::new();
    let msg = err(s.audition(json!({ "scope": "bus", "bus": "Nope", "kind": "reverb" })));
    assert!(
        msg.contains("reverb") && msg.contains("gain"),
        "the kind is the more useful thing to learn first: {msg}"
    );
}

/// Auditions are cached by what they contain, and a master audition is not
/// a track audition: the same effect on track 0 and on the master are
/// different sounds, so they must not share a clip.
#[test]
fn master_and_track_auditions_of_one_effect_are_different_clips() {
    let mut s = Session::new();
    let params = json!({ "ceiling_db": -6.0 });

    let on_track = ok(s.audition(json!({ "track": 0, "kind": "limiter", "params": params })));
    let on_master = ok(s.audition(json!({
        "scope": "master", "kind": "limiter", "params": params
    })));
    assert_eq!(on_track["cached"], false, "{on_track}");
    assert_eq!(on_master["cached"], false, "{on_master}");
    assert_ne!(on_track["path"], on_master["path"]);
    assert_ne!(clip(&on_track), clip(&on_master));

    // And the same master audition again is the cache hit it is on a track.
    let again = ok(s.audition(json!({
        "scope": "master", "kind": "limiter", "params": params
    })));
    assert_eq!(again["cached"], true, "{again}");
    assert_eq!(again["path"], on_master["path"]);
}

// ─── naming the chain ─────────────────────────────────────────────────

/// A call that does not name one chain is refused with what to pass, as
/// the chain tools refuse it. `track` is no longer required by the schema,
/// so the tool says it.
#[test]
fn an_audition_that_names_no_chain_says_how_to_name_one() {
    let mut s = Session::new();
    let nodes = s.node_count();

    let none = err(s.audition(json!({ "kind": "gain" })));
    assert!(
        none.contains("track") && none.contains("master") && none.contains("bus"),
        "{none}"
    );

    let both = err(s.audition(json!({ "scope": "master", "track": 0, "kind": "gain" })));
    assert!(both.contains("no track index"), "{both}");

    let track_only_scope = err(s.audition(json!({ "scope": "track", "kind": "gain" })));
    assert!(
        track_only_scope.contains("needs `track`"),
        "{track_only_scope}"
    );

    let bus_only_scope = err(s.audition(json!({ "scope": "bus", "kind": "gain" })));
    assert!(bus_only_scope.contains("needs `bus`"), "{bus_only_scope}");

    assert_eq!(s.node_count(), nodes);
    assert_eq!(s.cached_auditions(), 0);
}

/// The scope is an enum in the schema; one the tools do not have is turned
/// away before the tool runs.
#[test]
fn an_unknown_audition_scope_is_rejected_by_the_schema() {
    let mut s = Session::new();
    for scope in ["Master", "mix", ""] {
        match s.try_call("audition_effect", json!({ "scope": scope, "kind": "gain" })) {
            Err(DispatchError::SchemaValidation(m)) => {
                assert!(m.contains("scope"), "scope {scope:?}: {m}")
            }
            other => panic!("scope {scope:?} should fail the schema, got {other:?}"),
        }
    }
}

// ─── a track, as before ───────────────────────────────────────────────

/// `track` alone is a track audition and reads as it did: the result names
/// the track, `scope: "track"` spelled out is the same call (the same
/// clip), and a track past the end is refused as before.
#[test]
fn a_track_audition_means_what_it_meant() {
    let mut s = Session::new();
    let nodes = s.node_count();

    let plain = ok(s.audition(json!({ "track": 1, "kind": "gain", "params": { "db": -6.0 } })));
    assert_eq!(plain["track"], 1, "{plain}");
    assert!(
        plain["summary"].as_str().unwrap().contains("on track 1"),
        "{plain}"
    );
    assert_eq!(plain["cached"], false);

    let spelled = ok(s.audition(json!({
        "scope": "track", "track": 1, "kind": "gain", "params": { "db": -6.0 }
    })));
    assert_eq!(spelled["path"], plain["path"]);
    assert_eq!(spelled["cached"], true, "{spelled}");

    let msg = err(s.audition(json!({ "track": 9, "kind": "gain" })));
    assert!(msg.contains("track index 9 out of range"), "{msg}");
    assert_eq!(s.node_count(), nodes);
}

// ─── a bus ────────────────────────────────────────────────────────────

/// The same for a bus, since the five chain tools take one and an
/// audition that did not would be the one place `scope: "bus"` is refused.
/// Track 0 sends to it so that the effect is heard.
#[test]
fn a_bus_audition_is_what_add_effect_would_render() {
    let mut s = Session::new();
    let bus = s.create_bus("Drums");
    ok(s.call(
        "set_send",
        json!({ "track": 0, "bus_id": bus, "level_db": -6.0 }),
    ));
    let dry = s.render();
    let head = s.head();
    let nodes = s.node_count();

    let v = ok(s.audition(json!({
        "scope": "bus", "bus": "Drums", "kind": "gain", "params": { "db": 6.0 },
        "start_sec": 0.0, "end_sec": 1.0
    })));
    assert_eq!(s.head(), head);
    assert_eq!(s.node_count(), nodes);
    assert!(
        v["summary"].as_str().unwrap().contains("Drums"),
        "the result should name the bus: {v}"
    );
    let heard = clip(&v);

    ok(s.call(
        "add_effect",
        json!({ "scope": "bus", "bus": "Drums", "kind": "gain", "params": { "db": 6.0 } }),
    ));
    assert_eq!(heard, s.render());
    assert_ne!(heard, dry, "the audition is the session unchanged");
    // And what `add_effect` stored, and the audition therefore rendered,
    // is on the bus: not on a track, not on the master.
    let state = s.store.get(s.store.head().unwrap()).unwrap().state;
    assert_eq!(state.bus_routing.buses[0].effects.len(), 1);
    assert!(state.master_chain.is_empty());
    assert!(state.tracks.iter().all(|t| t.effects.is_empty()));

    // An unknown bus is refused with the buses there are; an unrunnable
    // kind with the kinds that work.
    let msg = err(s.audition(json!({ "scope": "bus", "bus": "Nope", "kind": "gain" })));
    assert!(msg.contains("Nope") && msg.contains("Drums"), "{msg}");
    let msg = err(s.audition(json!({ "scope": "bus", "bus": "Drums", "kind": "eq" })));
    assert!(msg.contains("eq") && msg.contains("limiter"), "{msg}");
}
