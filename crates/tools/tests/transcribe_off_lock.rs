//! `transcribe` runs without the store's lock, and an edit the user makes
//! while it runs is kept (#384, #421).
//!
//! A minute of CPU used to hold the store, engine and clipboard locks, and
//! every command that wanted one of them (a fader move, a graph read, undo)
//! waited for it. The tool runs on a staged store handle, with no lock held,
//! and its node is published only if the head did not move meanwhile. If the
//! user edited, the call is run again on the new head; this one finds the same
//! audio in the inference cache the first run filled, so the recogniser is not
//! run twice.
//!
//! The user is played from inside the recogniser, as `speech_tools_off_lock.rs`
//! does it: it takes the app's store the way a command would, and edits
//! through it. If the lock is held, `try_lock` fails and the test says so.

#[path = "support/speech.rs"]
mod speech;

use std::sync::{Arc, Mutex};

use serde_json::{json, Value};
use session::{NodeId, Store, TranscriptSource};
use speech::{tone, word, Fake};
use tempfile::TempDir;
use tools::tool::transcribe::{SpeechToText, TranscribeTool};
use tools::{Dispatched, Shared, ToolContext, ToolDispatcher, ToolResult};

const TARGET: usize = 0;
const OTHER: usize = 1;

fn user_edit(store: &mut Store, tool: &str, args: Value) {
    let mut engine = audio_engine::Engine::new();
    let mut clipboard = None;
    let mut ctx = ToolContext {
        store,
        engine: &mut engine,
        user_message: "",
        clipboard: &mut clipboard,
        allowed_tools: None,
    };
    match ToolDispatcher::default_dispatcher().invoke(tool, args, &mut ctx) {
        Ok(ToolResult::Ok(_)) => {}
        other => panic!("the user's {tool} failed: {other:?}"),
    }
}

struct Booth {
    _dir: TempDir,
    fake: Arc<Fake>,
    dispatcher: Mutex<ToolDispatcher>,
    store: Arc<Mutex<Store>>,
    engine: Mutex<audio_engine::Engine>,
    clipboard: Mutex<Option<tools::Clipboard>>,
}

impl Booth {
    fn new() -> Self {
        let dir = TempDir::new().expect("tempdir");
        let mut store = Store::open(dir.path()).expect("store");
        for (name, hz) in [("a.wav", 220.0), ("b.wav", 330.0)] {
            let path = speech::write_wav(&dir.path().join(name), &tone(2.0, hz));
            user_edit(&mut store, "load", json!({ "path": path }));
        }
        let store = Arc::new(Mutex::new(store));
        let fake = Arc::new(Fake::new(vec![
            word("Hello,", 0.3, 0.8),
            word("world.", 0.9, 1.4),
        ]));
        let mut dispatcher = ToolDispatcher::new();
        dispatcher.register(Box::new(TranscribeTool::with(
            Arc::clone(&fake) as Arc<dyn SpeechToText>
        )));
        Self {
            _dir: dir,
            fake,
            dispatcher: Mutex::new(dispatcher),
            store,
            engine: Mutex::new(audio_engine::Engine::new()),
            clipboard: Mutex::new(None),
        }
    }

    /// The user does `tool` from inside the recogniser's first run, through
    /// the app's store. Panics if the store's lock is held by the call.
    fn then_the_user(&self, tool: &'static str, args: Value) {
        let app = Arc::clone(&self.store);
        *self.fake.on_run.lock().unwrap() = Some(Box::new(move |run| {
            if run == 1 {
                let mut store = app.try_lock().unwrap_or_else(|_| {
                    panic!(
                        "transcribe holds the store's lock while the model runs, so the \
                         user's {tool} waits for it"
                    )
                });
                user_edit(&mut store, tool, args.clone());
            }
        }));
    }

    fn dispatched(&self, args: Value) -> Dispatched {
        Shared {
            dispatcher: &self.dispatcher,
            store: &self.store,
            engine: &self.engine,
            clipboard: &self.clipboard,
        }
        .dispatch("transcribe", args, "", None)
    }

    fn head(&self) -> NodeId {
        self.store.lock().unwrap().head().expect("a head")
    }

    fn node(&self, id: NodeId) -> session::SessionNode {
        self.store.lock().unwrap().get(id).expect("node")
    }

    fn node_count(&self) -> usize {
        self.store.lock().unwrap().list_nodes().unwrap().len()
    }
}

fn ok(result: &tools::Result<ToolResult>) -> &Value {
    match result {
        Ok(ToolResult::Ok(v)) => v,
        other => panic!("expected the call to succeed, got {other:?}"),
    }
}

fn error_text(result: &tools::Result<ToolResult>) -> &str {
    match result {
        Ok(ToolResult::Error(m)) => m,
        other => panic!("expected a refusal, got {other:?}"),
    }
}

#[test]
fn transcribe_is_one_of_the_tools_that_run_off_the_lock() {
    let d = ToolDispatcher::default_dispatcher();
    assert!(
        d.get("transcribe").expect("registered").runs_off_the_lock(),
        "transcribe runs under the store, engine and clipboard locks for the whole model run"
    );
}

/// With nothing else happening, the call is one run and one node.
#[test]
fn an_undisturbed_transcription_is_one_run_and_one_node() {
    let booth = Booth::new();
    let base = booth.head();

    let Dispatched { result, moved_head } = booth.dispatched(json!({ "track": TARGET }));

    let v = ok(&result);
    assert_eq!(v["cached"], false);
    assert_eq!(booth.fake.runs(), 1);
    assert_eq!(moved_head, Some(booth.head()));
    assert_eq!(booth.node(booth.head()).parent, Some(base));
    assert_eq!(booth.node_count(), 3);
}

/// The user moves a fader on the other track while the model runs. The
/// recogniser was not under the store's lock (the hook's `try_lock` would have
/// panicked), the fader move is kept, and the call is run again on top of it:
/// the second run is a cache hit, so the recogniser ran once in all and the
/// node is parented off the user's edit.
///
/// RED if `transcribe` does not say `runs_off_the_lock`: the user's edit,
/// made from inside the run, cannot take the lock.
#[test]
fn a_fader_moved_mid_run_is_kept_and_the_rerun_is_a_cache_hit() {
    let booth = Booth::new();
    let base = booth.head();
    booth.then_the_user("set_track_gain", json!({ "track": OTHER, "db": -6.0 }));

    let Dispatched { result, moved_head } = booth.dispatched(json!({ "track": TARGET }));

    let v = ok(&result);
    assert_eq!(
        v["cached"], true,
        "the call that was published is the re-run, and it answered from the cache: {v}"
    );
    assert_eq!(
        booth.fake.runs(),
        1,
        "the model ran once, not once per attempt"
    );

    let head = booth.head();
    assert_eq!(
        moved_head,
        Some(head),
        "the head the call reports is its own node"
    );
    let node = booth.node(head);
    let user = node.parent.expect("a parent");
    assert_ne!(user, base, "the node sits on top of the user's edit");
    assert_eq!(booth.node(user).parent, Some(base));
    assert_eq!(booth.node(user).state.tracks[OTHER].gain_db, -6.0);
    assert_eq!(
        node.state.tracks[OTHER].gain_db, -6.0,
        "the fader move is kept"
    );
    assert_eq!(
        node.state.transcript.as_ref().map(|t| t.words.len()),
        Some(2)
    );
    assert_eq!(
        node.state.transcript.unwrap().source,
        Some(TranscriptSource::Track {
            track: node.state.tracks[TARGET].id.clone()
        })
    );
    // load, load, the user's edit, the transcript: the attempt that lost
    // left no node.
    assert_eq!(booth.node_count(), 4);
}

/// The user cuts the very track being transcribed. The words are of audio
/// that is not there any more, so the call is refused, not run again, and the
/// user's edit stays.
#[test]
fn an_edit_to_the_transcribed_track_refuses_the_rerun() {
    let booth = Booth::new();
    booth.then_the_user(
        "cut_range",
        json!({ "track": TARGET, "start_sample": 8_000, "end_sample": 16_000 }),
    );

    let Dispatched { result, moved_head } = booth.dispatched(json!({ "track": TARGET }));

    let msg = error_text(&result);
    assert!(msg.contains("transcribe was not applied"), "{msg}");
    assert!(msg.contains("clips on track 0"), "{msg}");
    assert_eq!(booth.fake.runs(), 1);
    assert_eq!(moved_head, None);
    let state = booth.node(booth.head()).state;
    assert!(state.transcript.is_none(), "nothing was applied");
    assert_eq!(booth.node_count(), 3, "load, load, the user's cut");
}

/// A transcription of a file names no track, so nothing about the session
/// makes its re-run wrong: it is run again whatever the user edited.
#[test]
fn a_file_transcription_is_rerun_after_any_edit() {
    let booth = Booth::new();
    let file = booth._dir.path().join("a.wav");
    booth.then_the_user(
        "cut_range",
        json!({ "track": TARGET, "start_sample": 8_000, "end_sample": 16_000 }),
    );

    let Dispatched { result, .. } = booth.dispatched(json!({ "path": file.to_string_lossy() }));

    let v = ok(&result);
    assert_eq!(v["cached"], true);
    assert_eq!(booth.fake.runs(), 1);
    let t = booth
        .node(booth.head())
        .state
        .transcript
        .expect("transcript");
    assert_eq!(t.source, Some(TranscriptSource::File));
}
