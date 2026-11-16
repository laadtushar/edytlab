//! `tools::render::node`, the one way to render a node (ADR 0003, #714).
//!
//! These drive the module itself, with the store and engine the tools hold
//! and, for the window's way in, with the `Mutex`es it holds:
//!
//! * a node whose audio a sweep removed renders, and one that cannot be put
//!   back is refused;
//! * a cache hit reports the same left-out effects as the render that made
//!   the entry;
//! * a held entry survives an eviction that takes an unheld one;
//! * the cache's and the file's targets, the ways a render fails, and the
//!   locks the window's way in takes.
//!
//! The cache's cap is read from the environment, which is process-wide, so
//! every test takes [`Sandbox`].

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use audio_engine::{Engine, TimeRange};
use serde_json::json;
use session::{EffectInstance, NodeId, Store};
use tools::render::{self, Request, Span, Target};

#[path = "support/harness.rs"]
mod harness;
use harness::Session;

static ENV: Mutex<()> = Mutex::new(());
const CAP_VAR: &str = tools::preview_cache::CAP_ENV;
const TICK: Duration = Duration::from_millis(25);

/// A project holding a 1 s take and three edits to it, with the cache's cap
/// unset until a test sets it.
struct Sandbox {
    _lock: MutexGuard<'static, ()>,
    s: Session,
    /// The load, then each edit, oldest first.
    nodes: Vec<NodeId>,
}

impl Sandbox {
    fn new() -> Self {
        let lock = ENV.lock().unwrap_or_else(|p| p.into_inner());
        std::env::remove_var(CAP_VAR);
        let mut s = Session::with_take(1.0);
        let mut nodes = vec![s.head()];
        for start in [0.0, 0.3, 0.6] {
            s.ok(
                "silence_region",
                json!({ "track": 0, "start_sec": start, "end_sec": start + 0.1 }),
            );
            nodes.push(s.head());
        }
        Self {
            _lock: lock,
            s,
            nodes,
        }
    }

    fn cache(&self) -> tools::PreviewCache {
        tools::PreviewCache::new(self.s.store.project_dir())
    }

    fn entry(&self, node: NodeId) -> PathBuf {
        self.cache().path_for(node)
    }

    /// Render `node`'s whole mix into the cache.
    fn mix(
        &self,
        node: NodeId,
        hold: bool,
        verify: bool,
    ) -> Result<render::Rendered, render::Error> {
        render::node(
            &self.s.store,
            &self.s.engine,
            Request {
                node,
                span: Span::Whole,
                target: Target::Cache { hold, verify },
            },
        )
    }

    /// Sweep history at cap 0 and leave nothing a replay could start from:
    /// no take, and no older audio.
    fn strand(&self) {
        tools::reclaim::sweep(&self.s.store, 0).expect("sweep");
        std::fs::remove_file(self.s.dir.path().join("take.wav")).expect("remove the take");
        let head_audio: Vec<PathBuf> = self.s.state().tracks[0]
            .clips
            .iter()
            .map(|c| c.source_path.clone())
            .collect();
        let derived = self.s.dir.path().join(".audiograph").join("derived");
        for entry in std::fs::read_dir(derived).expect("derived").flatten() {
            if !head_audio.contains(&entry.path()) {
                std::fs::remove_file(entry.path()).expect("remove history's audio");
            }
        }
    }

    /// The same node with a stored effect the renderer cannot run, written
    /// the way an older build saved it.
    fn with_reverb(&mut self) -> NodeId {
        let mut state = self.s.state();
        state.tracks[0].effects.push(EffectInstance {
            kind: "reverb".into(),
            params: json!({}),
            bypassed: false,
        });
        self.s.append_state("older build", state);
        self.s.head()
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        std::env::remove_var(CAP_VAR);
    }
}

fn size(path: &Path) -> u64 {
    std::fs::metadata(path).expect("entry").len()
}

#[test]
fn a_swept_node_renders_and_its_audio_is_back() {
    let sb = Sandbox::new();
    let parent = sb.nodes[2];
    tools::reclaim::sweep(&sb.s.store, 0).expect("sweep");
    assert!(
        !tools::rederive::missing_paths(&sb.s.store, parent).is_empty(),
        "the sweep removed the node's audio, or this checks nothing"
    );

    let got = sb.mix(parent, false, true).expect("renders");

    assert!(!got.cached);
    assert!(got.path.is_file());
    assert!(got.hold.is_none());
    assert!(tools::rederive::missing_paths(&sb.s.store, parent).is_empty());
    let audio = got.audio.expect("a render reports what it wrote");
    assert_eq!((audio.sample_rate, audio.channels), (48_000, 1));
    assert!(audio.frames > 0 && audio.peak_dbfs.is_some());
}

#[test]
fn a_node_that_cannot_be_put_back_is_refused_and_leaves_no_file() {
    let sb = Sandbox::new();
    let parent = sb.nodes[2];
    sb.strand();

    let err = sb.mix(parent, false, true).expect_err("cannot be rebuilt");

    assert!(matches!(err, render::Error::MissingAudio(_)), "{err}");
    assert!(!sb.entry(parent).exists());
}

#[test]
fn a_node_that_is_not_there_is_a_node_error() {
    let sb = Sandbox::new();
    let err = sb
        .mix(NodeId([9; 32]), false, true)
        .expect_err("no such node");
    assert!(matches!(err, render::Error::Node(_)), "{err}");
}

#[test]
fn a_hit_reports_what_the_render_that_made_it_left_out() {
    let mut sb = Sandbox::new();
    let node = sb.with_reverb();

    let first = sb.mix(node, false, true).expect("renders");
    let second = sb.mix(node, false, true).expect("hits");

    assert!(!first.cached && second.cached);
    assert_eq!(first.path, second.path);
    assert_eq!(first.left_out, second.left_out);
    assert_eq!(first.left_out.len(), 1);
    assert_eq!(first.left_out[0].kind, "reverb");
    let state = sb.s.store.get(node).unwrap().state;
    // The one list is what the engine's own warnings say.
    assert_eq!(
        render::sentences(&second.left_out),
        audio_engine::render_warnings(&state)
    );
    assert_eq!(render::left_out(&state), second.left_out);
    // The cache holds the audio and not the report, so a hit knows no peak.
    assert!(first.audio.unwrap().peak_dbfs.is_some());
    let hit = second.audio.expect("read from the file");
    assert_eq!(hit.peak_dbfs, None);
    assert!(hit.frames > 0);
}

#[test]
fn a_clean_node_leaves_nothing_out() {
    let sb = Sandbox::new();
    assert!(sb
        .mix(sb.nodes[1], false, true)
        .unwrap()
        .left_out
        .is_empty());
}

#[test]
fn a_held_entry_survives_the_eviction_that_takes_an_unheld_one() {
    let sb = Sandbox::new();
    let [a, b, c] = [sb.nodes[1], sb.nodes[2], sb.nodes[3]];
    let one = {
        let r = sb.mix(a, false, true).unwrap();
        let bytes = size(&r.path);
        std::fs::remove_file(&r.path).unwrap();
        bytes
    };
    // Room for one render and a half.
    std::env::set_var(CAP_VAR, (one * 3 / 2).to_string());

    let held = sb.mix(a, true, true).expect("renders");
    assert!(held.hold.is_some());
    for node in [b, c] {
        std::thread::sleep(TICK);
        drop(sb.mix(node, false, true).expect("renders"));
    }

    assert!(sb.entry(a).is_file(), "the held entry was evicted");
    assert!(
        !sb.entry(b).exists(),
        "the unheld one should be what was given up"
    );

    // A hit is held too, and letting go counts it against the cap again.
    let again = sb.mix(a, true, true).expect("hit");
    assert!(again.cached && again.hold.is_some());
    drop((held, again));
    std::thread::sleep(TICK);
    drop(sb.mix(b, false, true).expect("renders"));
    assert!(!sb.entry(a).exists(), "released, it is evicted like any");
}

#[test]
fn an_excerpt_is_an_entry_of_its_own_and_never_the_mix() {
    let sb = Sandbox::new();
    let node = sb.nodes[1];
    let request = |span| Request {
        node,
        span,
        target: Target::Cache {
            hold: false,
            verify: true,
        },
    };
    let span = Span::Frames(TimeRange {
        start_frame: 0,
        end_frame: 1_000,
    });

    let excerpt = render::node(&sb.s.store, &sb.s.engine, request(span)).unwrap();
    assert_eq!(excerpt.audio.unwrap().frames, 1_000);
    assert_ne!(excerpt.path, sb.entry(node));
    assert!(!sb.entry(node).exists(), "the mix was never made");

    let again = render::node(&sb.s.store, &sb.s.engine, request(span)).unwrap();
    assert!(again.cached && again.path == excerpt.path);
    let whole = render::node(&sb.s.store, &sb.s.engine, request(Span::Whole)).unwrap();
    assert!(!whole.cached && whole.path == sb.entry(node));
}

#[test]
fn a_cached_file_that_is_not_a_wav_is_rendered_over_when_verified_and_served_when_not() {
    let sb = Sandbox::new();
    let node = sb.nodes[1];
    drop(sb.mix(node, false, true).unwrap());
    let entry = sb.entry(node);
    std::fs::write(&entry, b"marked").unwrap();

    let served = sb.mix(node, false, false).expect("served as it is");
    assert!(served.cached && served.audio.is_none());
    assert_eq!(std::fs::read(&entry).unwrap(), b"marked");

    let rendered = sb.mix(node, false, true).expect("rendered over");
    assert!(!rendered.cached && rendered.audio.is_some());
    assert_ne!(std::fs::read(&entry).unwrap(), b"marked");
}

#[test]
fn a_render_the_engine_refuses_is_a_render_error() {
    let sb = Sandbox::new();
    let node = sb.nodes[1];
    let backwards = Span::Frames(TimeRange {
        start_frame: 10,
        end_frame: 5,
    });

    let to_cache = render::node(
        &sb.s.store,
        &sb.s.engine,
        Request {
            node,
            span: backwards,
            target: Target::Cache {
                hold: false,
                verify: true,
            },
        },
    )
    .expect_err("invalid range");
    assert!(matches!(to_cache, render::Error::Render(_)), "{to_cache}");

    let out = sb.s.dir.path().join("out.wav");
    let to_file = render::node(
        &sb.s.store,
        &sb.s.engine,
        Request {
            node,
            span: backwards,
            target: Target::File {
                path: &out,
                destination: &out,
            },
        },
    )
    .expect_err("invalid range");
    assert!(matches!(to_file, render::Error::Render(_)), "{to_file}");
}

#[test]
fn a_cache_that_cannot_be_written_names_the_folder() {
    let sb = Sandbox::new();
    // A file where the cache's folder goes.
    let dir = sb.cache().dir().to_path_buf();
    std::fs::write(&dir, b"in the way").unwrap();

    let err = sb.mix(sb.nodes[1], false, true).expect_err("no cache");

    match &err {
        render::Error::Cache { dir: named, .. } => assert_eq!(named, &dir),
        other => panic!("expected a cache error, got {other}"),
    }
    assert!(err.to_string().contains("preview cache"), "{err}");
}

#[test]
fn a_file_target_writes_the_file_and_reports_what_it_left_out() {
    let mut sb = Sandbox::new();
    let node = sb.with_reverb();
    let out = sb.s.dir.path().join("export.wav");

    let got = render::node(
        &sb.s.store,
        &sb.s.engine,
        Request {
            node,
            span: Span::Frames(TimeRange {
                start_frame: 0,
                end_frame: 4_800,
            }),
            target: Target::File {
                path: &out,
                destination: &out,
            },
        },
    )
    .expect("renders");

    assert_eq!(got.path, out);
    assert!(out.is_file() && !got.cached && got.hold.is_none());
    assert_eq!(got.audio.unwrap().frames, 4_800);
    assert_eq!(got.left_out.len(), 1);
}

#[test]
fn a_destination_that_is_audio_the_project_reads_is_refused_untouched() {
    let sb = Sandbox::new();
    let take = sb.s.dir.path().join("take.wav");
    let before = std::fs::read(&take).unwrap();
    // The render goes to a scratch file and the destination is the take, as
    // `render_final` does for a compressed format.
    let scratch = sb.s.dir.path().join("scratch.wav");

    let err = render::node(
        &sb.s.store,
        &sb.s.engine,
        Request {
            node: sb.nodes[1],
            span: Span::Whole,
            target: Target::File {
                path: &scratch,
                destination: &take,
            },
        },
    )
    .expect_err("refused");

    assert!(matches!(err, render::Error::Refused(_)), "{err}");
    assert!(err.to_string().contains("take.wav"), "{err}");
    assert_eq!(std::fs::read(&take).unwrap(), before);
    assert!(!scratch.exists());
}

// ---------------------------------------------------------------------
// The window's way in: the locks
// ---------------------------------------------------------------------

#[test]
fn the_windows_locks_are_taken_one_at_a_time_and_released() {
    let sb = Sandbox::new();
    let node = sb.nodes[1];
    let store: Mutex<Store> = Mutex::new(Store::open(sb.s.store.project_dir()).unwrap());
    let engine = Mutex::new(Engine::new());

    let got = render::node(
        &store,
        &engine,
        Request {
            node,
            span: Span::Whole,
            target: Target::Cache {
                hold: true,
                verify: false,
            },
        },
    )
    .expect("renders through the mutexes");

    assert!(got.hold.is_some() && got.path == sb.entry(node));
    // Neither lock is still taken once it has returned.
    assert!(store.try_lock().is_ok() && engine.try_lock().is_ok());
}

#[test]
fn a_poisoned_lock_is_named() {
    let sb = Sandbox::new();
    let node = sb.nodes[1];
    let poisoned_store = Mutex::new(Store::open(sb.s.store.project_dir()).unwrap());
    let poisoned_engine = Mutex::new(Engine::new());
    let _ = std::thread::scope(|s| {
        s.spawn(|| {
            let _guard = poisoned_store.lock().unwrap();
            panic!("poison the store");
        })
        .join()
    });
    let _ = std::thread::scope(|s| {
        s.spawn(|| {
            let _guard = poisoned_engine.lock().unwrap();
            panic!("poison the engine");
        })
        .join()
    });
    let request = Request {
        node,
        span: Span::Whole,
        target: Target::Cache {
            hold: false,
            verify: false,
        },
    };

    let store_err =
        render::node(&poisoned_store, &sb.s.engine, request).expect_err("poisoned store");
    assert!(matches!(store_err, render::Error::Poisoned("store")));
    assert!(store_err.to_string().contains("poisoned"), "{store_err}");

    // A render that has to run takes the engine's lock; a hit does not.
    let engine_err =
        render::node(&sb.s.store, &poisoned_engine, request).expect_err("poisoned engine");
    assert!(matches!(engine_err, render::Error::Poisoned("engine")));
    assert!(!sb.entry(node).exists(), "a failed render leaves no entry");
    drop(sb.mix(node, false, true).unwrap());
    assert!(render::node(&sb.s.store, &poisoned_engine, request).is_ok());

    let out = sb.s.dir.path().join("poisoned.wav");
    let file_err = render::node(
        &sb.s.store,
        &poisoned_engine,
        Request {
            node,
            span: Span::Whole,
            target: Target::File {
                path: &out,
                destination: &out,
            },
        },
    )
    .expect_err("poisoned engine");
    assert!(matches!(file_err, render::Error::Poisoned("engine")));
}

// ---------------------------------------------------------------------
// The tools' answers when the module refuses
// ---------------------------------------------------------------------

#[test]
fn the_preview_tool_words_each_refusal_as_before() {
    let mut sb = Sandbox::new();
    let missing = sb.s.err(
        "render_preview",
        json!({ "node_id": NodeId([9; 32]).to_hex() }),
    );
    assert!(missing.starts_with("node lookup failed: "), "{missing}");

    let parent = sb.nodes[2];
    sb.strand();
    let stranded =
        sb.s.err("render_preview", json!({ "node_id": parent.to_hex() }));
    assert!(!stranded.starts_with("render failed"), "{stranded}");
    assert!(!sb.entry(parent).exists());
}

#[test]
fn the_export_tool_words_a_failed_render_as_before() {
    let mut sb = Sandbox::new();
    // A node with no tracks: the engine has nothing to render.
    let mut state = sb.s.state();
    state.tracks.clear();
    sb.s.append_state("empty", state);
    let out = sb.s.dir.path().join("empty.wav");

    let msg = sb.s.err(
        "render_final",
        json!({ "node_id": sb.s.head_hex(), "format": "wav", "out_path": out }),
    );

    assert!(msg.starts_with("render failed: "), "{msg}");
}
