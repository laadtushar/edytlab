//! `select_region` and `duck_under_speech` run off the store lock (#570).
//!
//! With `voice_tracks`, both decode every clip of the voice tracks and scan
//! the whole timeline for speech (`speech_detect::speech_in_track`). That
//! takes as long as the voice is long, and while they ran under the store,
//! engine and clipboard locks every command that needed one of them waited
//! for it: a fader move, a graph read, a preview. They now run on a staged
//! store handle, as the other analysis tools do (`Shared::dispatch`, #421).
//!
//! Two kinds of test:
//!
//! * **During the analysis** (Unix). The voice clip's source is swapped for
//!   a FIFO, so the tool opens it and waits inside `decode_file`, in the
//!   middle of its analysis, until the test lets the audio through. While it
//!   waits the test moves a fader the way the mixer does (`run_track_tool`:
//!   the store, the engine and the dispatcher, in that order). Under the
//!   locks the fader move cannot happen until the analysis ends.
//! * **The rules for an edit that lands mid-run** (every platform). A
//!   wrapper makes the user's edit from inside the tool's run, through the
//!   app's store, as `off_lock_dispatch.rs` does. A read is answered from
//!   the head it began on and is never run again; a duck is run again on
//!   top of an edit that leaves the tracks it names alone, and is refused
//!   when the user changed one of them, since its arguments no longer
//!   describe the session.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use hound::{SampleFormat, WavSpec, WavWriter};
use serde_json::{json, Value};
use session::{NodeId, Store};
use tempfile::TempDir;
use tools::tool::{DuckUnderSpeechTool, SelectRegionTool};
use tools::{Dispatched, Shared, Tool, ToolContext, ToolDispatcher, ToolResult};

const SR: u32 = 48_000;
const MUSIC: usize = 0;
const VOICE: usize = 1;

/// Mono 16-bit WAV of `samples`.
fn write_samples(path: &Path, samples: &[f32]) -> PathBuf {
    let spec = WavSpec {
        channels: 1,
        sample_rate: SR,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut w = WavWriter::create(path, spec).expect("wav writer");
    for s in samples {
        w.write_sample((s.clamp(-1.0, 1.0) * 32_767.0) as i16)
            .unwrap();
    }
    w.finalize().unwrap();
    path.to_path_buf()
}

/// Voiced sound: six harmonics of 140 Hz falling as 1/k, about -21 dBFS,
/// the synthetic voice `select_region.rs` and `ducking.rs` use.
fn voice_burst(seconds: f64) -> Vec<f32> {
    (0..(SR as f64 * seconds) as usize)
        .map(|n| {
            let t = n as f64 / SR as f64;
            (1..=6)
                .map(|k| 0.1 / k as f64 * (std::f64::consts::TAU * k as f64 * 140.0 * t).sin())
                .sum::<f64>() as f32
        })
        .collect()
}

/// Ten seconds of voice: two lines, at 1-2 s and 5-6 s, in silence.
fn two_lines() -> Vec<f32> {
    let mut buf = vec![0.0f32; SR as usize * 10];
    for at in [1.0, 5.0] {
        let start = (at * SR as f64) as usize;
        for (d, s) in buf[start..].iter_mut().zip(voice_burst(1.0)) {
            *d = s;
        }
    }
    buf
}

/// Ten seconds of a steady sine: music, which the detector does not read
/// as speech.
fn music() -> Vec<f32> {
    (0..SR as usize * 10)
        .map(|n| (std::f32::consts::TAU * 440.0 * n as f32 / SR as f32).sin() * 0.25)
        .collect()
}

// ---------------------------------------------------------------------
// The user, mid-run
// ---------------------------------------------------------------------

/// The user's edits left to make, a real tool and its arguments, one at the
/// start of each run; and how many runs there have been.
struct Script {
    edits: Mutex<VecDeque<(&'static str, Value)>>,
    runs: AtomicUsize,
}

/// A real tool that, before it runs, plays the part of the user: it takes
/// the app's store the way a command does, and edits through it. If the
/// store's lock is held, the call is running under it.
struct Hooked {
    inner: Box<dyn Tool>,
    app_store: Arc<Mutex<Store>>,
    script: Arc<Script>,
}

impl Tool for Hooked {
    fn name(&self) -> &'static str {
        self.inner.name()
    }
    fn schema(&self) -> Value {
        self.inner.schema()
    }
    fn mutates(&self) -> bool {
        self.inner.mutates()
    }
    fn runs_off_the_lock(&self) -> bool {
        self.inner.runs_off_the_lock()
    }
    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> tools::Result<ToolResult> {
        self.script.runs.fetch_add(1, Ordering::SeqCst);
        let edit = self.script.edits.lock().unwrap().pop_front();
        if let Some((name, user_args)) = edit {
            let mut app = self.app_store.try_lock().unwrap_or_else(|_| {
                panic!(
                    "{} holds the store's lock while it runs, so the user's {name} waits for it",
                    self.inner.name()
                )
            });
            user_edit(&mut app, name, user_args);
        }
        self.inner.invoke(args, ctx)
    }
}

/// `tool` under the store the caller holds, with an engine and clipboard of
/// its own: a command's edit.
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

// ---------------------------------------------------------------------
// The app's state
// ---------------------------------------------------------------------

/// A session of two tracks, music then voice, one clip each, with the two
/// speech tools registered under the user's hook.
struct Booth {
    _dir: TempDir,
    /// The voice clip's source file.
    voice: PathBuf,
    dispatcher: Mutex<ToolDispatcher>,
    store: Arc<Mutex<Store>>,
    engine: Mutex<audio_engine::Engine>,
    clipboard: Mutex<Option<tools::Clipboard>>,
    script: Arc<Script>,
}

impl Booth {
    fn new() -> Self {
        let dir = TempDir::new().expect("tempdir");
        let music = write_samples(&dir.path().join("music.wav"), &music());
        let voice = write_samples(&dir.path().join("voice.wav"), &two_lines());
        let mut store = Store::open(dir.path()).expect("store");
        for path in [&music, &voice] {
            user_edit(&mut store, "load", json!({ "path": path }));
        }
        let store = Arc::new(Mutex::new(store));

        let script = Arc::new(Script {
            edits: Mutex::new(VecDeque::new()),
            runs: AtomicUsize::new(0),
        });
        let mut dispatcher = ToolDispatcher::new();
        let speech_tools: [Box<dyn Tool>; 2] =
            [Box::new(SelectRegionTool), Box::new(DuckUnderSpeechTool)];
        for inner in speech_tools {
            dispatcher.register(Box::new(Hooked {
                inner,
                app_store: Arc::clone(&store),
                script: Arc::clone(&script),
            }));
        }
        let booth = Self {
            _dir: dir,
            voice,
            dispatcher: Mutex::new(dispatcher),
            store,
            engine: Mutex::new(audio_engine::Engine::new()),
            clipboard: Mutex::new(None),
            script,
        };
        let state = booth.state(booth.head());
        assert_eq!(state.tracks.len(), 2);
        assert_eq!(state.tracks[VOICE].clips[0].source_path, booth.voice);
        booth
    }

    /// The user calls `tool` from inside the speech tool's next run.
    fn then_the_user(&self, tool: &'static str, args: Value) {
        self.script.edits.lock().unwrap().push_back((tool, args));
    }

    fn dispatched(&self, tool: &str, args: Value) -> Dispatched {
        Shared {
            dispatcher: &self.dispatcher,
            store: &self.store,
            engine: &self.engine,
            clipboard: &self.clipboard,
        }
        .dispatch(tool, args, "", None)
    }

    fn head(&self) -> NodeId {
        self.store.lock().unwrap().head().expect("a head")
    }

    fn node(&self, id: NodeId) -> session::SessionNode {
        self.store.lock().unwrap().get(id).expect("node")
    }

    fn state(&self, id: NodeId) -> session::SessionState {
        self.node(id).state
    }

    fn node_count(&self) -> usize {
        self.store.lock().unwrap().list_nodes().unwrap().len()
    }

    /// How many times a wrapped tool ran.
    fn runs(&self) -> usize {
        self.script.runs.load(Ordering::SeqCst)
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

fn near(v: &Value, key: &str, want: f64) {
    let got = v[key].as_f64().unwrap_or_else(|| panic!("no {key} in {v}"));
    assert!(
        (got - want).abs() <= 0.1,
        "{key}: wanted {want}, got {got}: {v}"
    );
}

#[test]
fn both_speech_tools_run_off_the_lock() {
    let d = ToolDispatcher::default_dispatcher();
    for name in ["select_region", "duck_under_speech"] {
        assert!(
            d.get(name).expect("registered").runs_off_the_lock(),
            "{name} runs under the store, engine and clipboard locks for the whole of a \
             voice-track analysis"
        );
    }
}

// ---------------------------------------------------------------------
// An edit that lands mid-run
// ---------------------------------------------------------------------

/// A read is answered from the head it began on, the user's fader move is
/// kept, and the call is not run again: it appends nothing, so there is
/// nothing to publish on top of the user's edit.
///
/// RED on main: `select_region` ran under the store's lock, so the user's
/// edit, made from inside the run, could not take it.
#[test]
fn select_region_answers_while_the_user_moves_a_fader_and_is_not_run_again() {
    let booth = Booth::new();
    let base = booth.head();
    booth.then_the_user("set_track_gain", json!({ "track": VOICE, "db": -6.0 }));

    let Dispatched { result, moved_head } = booth.dispatched(
        "select_region",
        json!({ "speech_passage": -1, "voice_tracks": [VOICE] }),
    );

    let v = ok(&result);
    near(v, "start_sec", 5.0);
    near(v, "end_sec", 6.0);
    assert_eq!(v["keyed_on"], "audio", "{v}");
    assert_eq!(booth.runs(), 1, "a read is not run again");
    assert_eq!(
        moved_head, None,
        "the user's edit is not the call's to report"
    );

    let head = booth.head();
    assert_ne!(head, base, "the user's edit landed");
    assert_eq!(booth.node(head).parent, Some(base));
    assert_eq!(booth.state(head).tracks[VOICE].gain_db, -6.0);
    assert_eq!(booth.node_count(), 3, "load, load, and the user's edit");
}

/// The duck loses the race to a fader move on the music track. A fader is
/// not the music's clips, so the call still means what it meant, and it is
/// run again on top of the user's edit: the gain is kept, the automation is
/// written, and the head the call reports is its own node.
///
/// RED on main: `duck_under_speech` ran under the store's lock.
#[test]
fn duck_under_speech_is_run_again_on_top_of_a_fader_move_and_keeps_it() {
    let booth = Booth::new();
    let base = booth.head();
    booth.then_the_user("set_track_gain", json!({ "track": MUSIC, "db": -6.0 }));

    let Dispatched { result, moved_head } = booth.dispatched(
        "duck_under_speech",
        json!({ "music_track": MUSIC, "voice_tracks": [VOICE] }),
    );

    let v = ok(&result);
    assert_eq!(v["passages"], 2, "{v}");
    assert_eq!(v["keyed_on"], "audio", "{v}");
    assert_eq!(
        booth.runs(),
        2,
        "lost the race once, then ran on the new head"
    );

    let head = booth.head();
    assert_eq!(moved_head, Some(head), "the call reports the head it made");
    let user = booth.node(head).parent.expect("a parent");
    assert_ne!(user, base);
    assert_eq!(
        booth.node(user).parent,
        Some(base),
        "the parent is the user's edit"
    );
    let state = booth.state(head);
    assert_eq!(
        state.tracks[MUSIC].gain_db, -6.0,
        "the user's fader is kept"
    );
    assert!(
        !state.tracks[MUSIC].clips[0].volume_envelope.is_empty(),
        "the duck was written"
    );
    assert_eq!(
        booth.node_count(),
        4,
        "load, load, the user's edit and one duck: the lost run left no node"
    );
}

/// The user moved the voice clip while the duck read it: the speech is no
/// longer where the first run found it, so the call is refused rather than
/// run again on audio its arguments did not describe. The user's move stays.
///
/// RED on main: `duck_under_speech` ran under the store's lock.
#[test]
fn duck_under_speech_is_refused_when_the_voice_clip_moves_while_it_runs() {
    let booth = Booth::new();
    booth.then_the_user(
        "move_clip",
        json!({ "track": VOICE, "clip_index": 0, "start_sec": 2.0 }),
    );

    let Dispatched { result, moved_head } = booth.dispatched(
        "duck_under_speech",
        json!({ "music_track": MUSIC, "voice_tracks": [VOICE] }),
    );

    let msg = error_text(&result);
    assert!(msg.contains("duck_under_speech was not applied"), "{msg}");
    assert!(msg.contains("the clips on track 1"), "{msg}");
    assert_eq!(booth.runs(), 1, "refused before a second run");
    assert_eq!(moved_head, None);

    let state = booth.state(booth.head());
    assert_eq!(
        state.tracks[VOICE].clips[0].start_in_track,
        2 * SR as u64,
        "the user's move stays"
    );
    assert!(
        state.tracks[MUSIC].clips[0].volume_envelope.is_empty(),
        "no duck was written"
    );
    assert_eq!(booth.node_count(), 3, "load, load, and the user's move");
}

/// The user drew automation on the music clip while the duck ran. A re-run
/// would overwrite it with the duck's curve, so the call is refused, and the
/// user's curve is what the music has.
///
/// RED on main: `duck_under_speech` ran under the store's lock.
#[test]
fn duck_under_speech_is_refused_when_the_user_draws_on_the_music_while_it_runs() {
    let booth = Booth::new();
    booth.then_the_user(
        "set_clip_envelope",
        json!({
            "track_index": MUSIC,
            "clip_index": 0,
            "points": [{ "time_sec": 0.0, "gain_db": -3.0 }, { "time_sec": 4.0, "gain_db": 0.0 }],
        }),
    );

    let Dispatched { result, moved_head } = booth.dispatched(
        "duck_under_speech",
        json!({ "music_track": MUSIC, "voice_tracks": [VOICE] }),
    );

    let msg = error_text(&result);
    assert!(msg.contains("the clips on track 0"), "{msg}");
    assert_eq!(moved_head, None);
    let envelope = &booth.state(booth.head()).tracks[MUSIC].clips[0].volume_envelope;
    assert_eq!(
        envelope.len(),
        2,
        "the user's two points, not the duck's curve"
    );
    assert_eq!(envelope[0].gain_db, -3.0);
}

// ---------------------------------------------------------------------
// During the analysis
// ---------------------------------------------------------------------

#[cfg(unix)]
mod during_the_analysis {
    use std::fs::{File, OpenOptions};
    use std::io::Write;
    use std::path::PathBuf;
    use std::sync::mpsc::{channel, RecvTimeoutError};
    use std::thread::ScopedJoinHandle;
    use std::time::{Duration, Instant};

    use super::*;

    /// How long a fader move waits for the locks before the test calls it
    /// stuck. Off the lock it gets them at the first try.
    const FADER_PATIENCE: Duration = Duration::from_secs(2);

    /// A clip source whose audio arrives only when the test lets it through.
    ///
    /// The WAV is swapped for a FIFO at the same path. A tool that decodes
    /// the clip opens it and then waits in `read_to_end`, inside its
    /// analysis, until [`HeldAudio::release`] writes the bytes and closes
    /// the other end.
    struct HeldAudio {
        path: PathBuf,
        wav: Vec<u8>,
    }

    impl HeldAudio {
        fn hold(path: &Path) -> Self {
            let wav = std::fs::read(path).expect("the wav");
            std::fs::remove_file(path).expect("remove the wav");
            let made = std::process::Command::new("mkfifo")
                .arg(path)
                .status()
                .expect("run mkfifo");
            assert!(made.success(), "mkfifo {}", path.display());
            Self {
                path: path.to_path_buf(),
                wav,
            }
        }

        /// The writing end, once a reader has opened the FIFO: the moment
        /// the tool is inside its analysis. `None` if `call` finished
        /// first, without reading the audio at all.
        fn wait_for_reader<T>(&self, call: &ScopedJoinHandle<'_, T>) -> Option<File> {
            let (tx, rx) = channel();
            let path = self.path.clone();
            // Opening a FIFO to write blocks until someone opens it to read.
            // Detached, so a call that never reads it leaves this thread
            // parked instead of the test.
            std::thread::spawn(move || {
                let _ = tx.send(OpenOptions::new().write(true).open(&path));
            });
            let give_up = Instant::now() + Duration::from_secs(60);
            loop {
                match rx.recv_timeout(Duration::from_millis(20)) {
                    Ok(writer) => return Some(writer.expect("open the fifo to write")),
                    Err(RecvTimeoutError::Timeout)
                        if !call.is_finished() && Instant::now() < give_up => {}
                    Err(_) => return None,
                }
            }
        }

        /// Let the analysis go on. A plain file with the same audio takes the
        /// path first, for any later run of the call (the FIFO stays open to
        /// the run that is reading it), then that run gets its bytes.
        fn release(self, mut writer: File) {
            let plain = self.path.with_extension("wav.part");
            std::fs::write(&plain, &self.wav).expect("write the plain copy");
            std::fs::rename(&plain, &self.path).expect("put it in place");
            writer.write_all(&self.wav).expect("feed the fifo");
        }
    }

    impl Booth {
        /// A fader move as the mixer makes one (`run_track_tool`): the store,
        /// the engine and the dispatcher, in that order, then the edit.
        /// Tried until [`FADER_PATIENCE`] runs out; the new head, or `None`
        /// if a lock stayed taken.
        fn fader_move(&self, track: usize, db: f32) -> Option<NodeId> {
            let deadline = Instant::now() + FADER_PATIENCE;
            loop {
                if let Some(head) = self.try_fader_move(track, db) {
                    return Some(head);
                }
                if Instant::now() >= deadline {
                    return None;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        }

        fn try_fader_move(&self, track: usize, db: f32) -> Option<NodeId> {
            let mut store = self.store.try_lock().ok()?;
            let _engine = self.engine.try_lock().ok()?;
            let _dispatcher = self.dispatcher.try_lock().ok()?;
            user_edit(
                &mut store,
                "set_track_gain",
                json!({ "track": track, "db": db }),
            );
            store.head()
        }

        /// Dispatch `tool` with the voice's audio held, move the music's
        /// fader while the tool waits for it, then let the audio through.
        /// The call, and the head the fader move made if it could be made.
        fn fader_move_during(&self, tool: &str, args: Value) -> (Dispatched, Option<NodeId>) {
            let held = HeldAudio::hold(&self.voice);
            std::thread::scope(|scope| {
                let call = scope.spawn(|| self.dispatched(tool, args));
                let Some(writer) = held.wait_for_reader(&call) else {
                    let call = call.join().expect("the call");
                    panic!("{tool} never read the voice's audio: {:?}", call.result);
                };
                // The tool is inside `decode_file`, and cannot finish its
                // analysis until the audio is let through.
                let user = self.fader_move(MUSIC, -6.0);
                held.release(writer);
                (call.join().expect("the call"), user)
            })
        }
    }

    /// **The issue's case.** While `select_region` reads a voice track, the
    /// mixer gets the store, the engine and the dispatcher at once.
    ///
    /// RED on main: the fader move waited for the whole analysis, holding
    /// out past `FADER_PATIENCE`, because the store and engine locks were
    /// held for it.
    #[test]
    fn a_fader_moves_while_select_region_reads_a_voice_track() {
        let booth = Booth::new();
        let base = booth.head();

        let (call, user) = booth.fader_move_during(
            "select_region",
            json!({ "speech_passage": -1, "voice_tracks": [VOICE] }),
        );

        let user = user.expect(
            "the fader move waited for select_region's analysis of the voice track: the store, \
             engine or dispatcher lock was held while it read the audio",
        );
        let v = ok(&call.result);
        near(v, "start_sec", 5.0);
        near(v, "end_sec", 6.0);
        assert_eq!(call.moved_head, None, "a read moves nothing");
        assert_eq!(booth.head(), user, "the user's edit is the head");
        assert_eq!(booth.node(user).parent, Some(base));
    }

    /// The same for `duck_under_speech`, which also appends: the fader move
    /// lands mid-analysis, the duck loses the race, and its node is made on
    /// top of the user's edit, which is kept.
    ///
    /// RED on main, as above.
    #[test]
    fn a_fader_moves_while_duck_under_speech_reads_a_voice_track() {
        let booth = Booth::new();
        let base = booth.head();

        let (call, user) = booth.fader_move_during(
            "duck_under_speech",
            json!({ "music_track": MUSIC, "voice_tracks": [VOICE] }),
        );

        let user = user.expect(
            "the fader move waited for duck_under_speech's analysis of the voice track: the \
             store, engine or dispatcher lock was held while it read the audio",
        );
        let v = ok(&call.result);
        assert_eq!(v["passages"], 2, "{v}");
        let head = booth.head();
        assert_eq!(call.moved_head, Some(head));
        assert_eq!(
            booth.node(head).parent,
            Some(user),
            "made on top of the edit"
        );
        assert_eq!(booth.node(user).parent, Some(base));
        let state = booth.state(head);
        assert_eq!(
            state.tracks[MUSIC].gain_db, -6.0,
            "the user's fader is kept"
        );
        assert!(!state.tracks[MUSIC].clips[0].volume_envelope.is_empty());
    }
}
