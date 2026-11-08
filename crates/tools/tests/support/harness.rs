//! The session every tool test drives, and the WAVs it loads.
//!
//! A tool test needs the same four things the app has: a project folder, a
//! `Store` over it, an `Engine` and a `ToolDispatcher`. Each test used to
//! build them itself, with a `call` that wrapped them in a `ToolContext`.
//! [`Session`] is that one copy. A test binary includes it with
//! `#[path = "support/harness.rs"] mod harness;` and uses a subset.
//!
//! A test that needs a method of its own adds it in an `impl Session`
//! block in its own file (the type is in the test's crate). A test that
//! deliberately opens its store another way (`Store::open_private`, a
//! registry double) builds that store itself and does not use this.
#![allow(dead_code)]

use std::ops::{Deref, DerefMut};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};

use hound::{SampleFormat, WavReader, WavSpec, WavWriter};
use serde_json::{json, Value};
use session::{
    Clip, EffectInstance, NodeId, SessionNode, SessionState, Store, Transcript, TranscriptSource,
    TranscriptWord,
};
use tempfile::TempDir;
use tools::{Clipboard, Tool, ToolContext, ToolDispatcher, ToolResult};

/// A project in a temp folder, with the engine and dispatcher the app uses.
pub struct Session {
    /// Kept alive for the whole test: dropping it deletes the project the
    /// store is writing into. Tests also write their source audio here.
    pub dir: TempDir,
    pub store: Store,
    pub engine: audio_engine::Engine,
    pub dispatcher: ToolDispatcher,
    pub clipboard: Option<Clipboard>,
}

impl Session {
    /// An empty project: no tracks, no audio.
    pub fn empty() -> Self {
        let dir = TempDir::new().expect("tempdir");
        let project = dir.path().to_path_buf();
        Self::around(dir, &project)
    }

    /// An empty project in the folder `sub` of the temp folder, so the rest
    /// of the temp folder is free for files that are not the project's.
    pub fn in_subfolder(sub: &str) -> Self {
        let dir = TempDir::new().expect("tempdir");
        let project = dir.path().join(sub);
        std::fs::create_dir_all(&project).unwrap();
        Self::around(dir, &project)
    }

    fn around(dir: TempDir, project: &Path) -> Self {
        let store = Store::open(project).expect("open store");
        Self {
            dir,
            store,
            engine: audio_engine::Engine::new(),
            dispatcher: ToolDispatcher::default_dispatcher(),
            clipboard: None,
        }
    }

    /// One mono track per `(sample_rate, samples)`, in order.
    pub fn with_tracks(tracks: &[(u32, Vec<f32>)]) -> Self {
        let mut s = Self::empty();
        for (i, (rate, samples)) in tracks.iter().enumerate() {
            let path = s.dir.path().join(format!("track{i}.wav"));
            write_frames(&path, *rate, 1, samples.len(), |n| samples[n]);
            s.load(&path);
        }
        assert_eq!(s.state().tracks.len(), tracks.len(), "one track each");
        s
    }

    /// A project with one track per `(freq, amp)`: a sine of `frames` frames
    /// at `rate`, loaded in order.
    pub fn with_tones(rate: u32, frames: usize, tones: &[(f32, f32)]) -> Self {
        let mut s = Self::empty();
        for (i, (freq, amp)) in tones.iter().enumerate() {
            let src = write_sine(
                &s.dir.path().join(format!("tone{i}.wav")),
                rate,
                *freq,
                *amp,
                frames,
            );
            s.load(&src);
        }
        s
    }

    /// `with_sine(48_000, 1)` and `edits` *destructive* edits to it. `gain`
    /// only records a track gain and writes no audio, so each edit is a
    /// `silence_region`, longer than the last so that every edit produces
    /// distinct samples (repeating one would hit the content-addressed
    /// store and write no new file).
    pub fn with_edits(edits: usize) -> Self {
        let mut s = Self::with_sine(48_000, 1);
        for i in 0..edits {
            let end = 0.1 * (i as f64 + 1.0);
            s.ok(
                "silence_region",
                json!({ "track": 0, "start_sec": 0.0, "end_sec": end }),
            );
        }
        s
    }

    /// A project with one track: `take.wav`, `seconds` of a 440 Hz sine at
    /// 8000/32767 of full scale, at 48 kHz, loaded.
    pub fn with_take(seconds: f64) -> Self {
        let mut s = Self::empty();
        let take = write_secs(&s.dir.path().join("take.wav"), seconds, 440.0);
        s.load(&take);
        s
    }

    /// A project with one track: `seconds` of a 440 Hz sine at a quarter of
    /// full scale, at `rate`, written as `in.wav` in the project folder and
    /// loaded.
    pub fn with_sine(rate: u32, seconds: usize) -> Self {
        let mut s = Self::empty();
        let src = write_tone(&s.dir.path().join("in.wav"), rate, seconds);
        s.load(&src);
        s
    }

    /// Run a tool as the agent would. The dispatcher's own error (an unknown
    /// tool, a refused argument) is returned, not unwrapped.
    pub fn try_call(&mut self, tool: &str, args: Value) -> tools::Result<ToolResult> {
        self.try_call_saying(tool, args, "")
    }

    /// As `try_call`, from a chat message, which some tools read their
    /// range from (`[apply to 0:01-0:02]`, as the app adds for a selection).
    pub fn try_call_saying(
        &mut self,
        tool: &str,
        args: Value,
        message: &str,
    ) -> tools::Result<ToolResult> {
        let mut ctx = ToolContext {
            store: &mut self.store,
            engine: &mut self.engine,
            user_message: message,
            clipboard: &mut self.clipboard,
            allowed_tools: None,
        };
        self.dispatcher.invoke(tool, args, &mut ctx)
    }

    /// Run a tool and unwrap the dispatcher's answer: the tool's own `Ok`
    /// or `Error` is the result.
    pub fn call(&mut self, tool: &str, args: Value) -> ToolResult {
        self.call_saying(tool, args, "")
    }

    pub fn call_saying(&mut self, tool: &str, args: Value, message: &str) -> ToolResult {
        self.try_call_saying(tool, args, message)
            .unwrap_or_else(|e| panic!("{tool} was rejected before it ran: {e}"))
    }

    /// The result of a tool that must succeed.
    pub fn ok(&mut self, tool: &str, args: Value) -> Value {
        match self.call(tool, args) {
            ToolResult::Ok(v) => v,
            ToolResult::Error(m) => panic!("{tool}: expected Ok, got Error({m})"),
        }
    }

    /// The message of a tool that must refuse.
    pub fn err(&mut self, tool: &str, args: Value) -> String {
        match self.call(tool, args) {
            ToolResult::Error(m) => m,
            ToolResult::Ok(v) => panic!("{tool}: expected Error, got Ok({v})"),
        }
    }

    /// `load` a file onto a new track.
    pub fn load(&mut self, path: &Path) -> Value {
        self.ok("load", json!({ "path": path.to_string_lossy() }))
    }

    /// Create a bus; returns its id.
    pub fn create_bus(&mut self, name: &str) -> String {
        let v = self.ok("create_bus", json!({ "name": name }));
        v["bus_id"].as_str().expect("a bus id").to_string()
    }

    /// Render the head to a WAV in the project folder (all of it, or
    /// `range`) and return its 16-bit samples.
    pub fn render_pcm(&self, range: Option<audio_engine::TimeRange>) -> Vec<i16> {
        static RENDERS: AtomicUsize = AtomicUsize::new(0);
        let n = RENDERS.fetch_add(1, Ordering::Relaxed);
        let out = self.dir.path().join(format!("out-{n}.wav"));
        audio_engine::render_state_to_wav(&self.state(), &out, range).expect("render");
        read_pcm(&out)
    }

    pub fn head(&self) -> NodeId {
        self.store.head().expect("a head")
    }

    /// The head as the hex the tools print.
    pub fn head_hex(&self) -> String {
        self.head().to_hex()
    }

    pub fn state(&self) -> SessionState {
        self.store.get(self.head()).expect("head node").state
    }

    pub fn node_count(&self) -> usize {
        self.store.list_nodes().expect("list nodes").len()
    }

    /// Append `state` as a node on the head, as an edit outside the tools
    /// would.
    pub fn append_state(&mut self, label: &str, state: SessionState) {
        let node = SessionNode {
            id: NodeId([0u8; 32]),
            parent: None,
            created_at: chrono::Utc::now(),
            label: Some(label.into()),
            reasoning: None,
            state,
            op: None,
        };
        self.store.append(node).expect("append node");
    }

    /// `transcribe` needs a model a test cannot rely on, so the transcript
    /// is written directly, in the shape it produces. This one is of no
    /// particular source.
    pub fn with_transcript(&mut self, words: &[(&str, f32, f32)]) {
        self.set_transcript(None, words);
    }

    /// The same, as `transcribe` leaves it for track `track`: words in
    /// session seconds.
    pub fn with_track_transcript(&mut self, track: usize, words: &[(&str, f32, f32)]) {
        let id = self.state().tracks[track].id.clone();
        self.set_transcript(Some(TranscriptSource::Track { track: id }), words);
    }

    fn set_transcript(&mut self, source: Option<TranscriptSource>, words: &[(&str, f32, f32)]) {
        let mut state = self.state();
        state.transcript = Some(Transcript {
            source,
            words: words
                .iter()
                .map(|(text, start, end)| TranscriptWord {
                    text: (*text).to_string(),
                    start_s: *start,
                    end_s: *end,
                    confidence: 0.9,
                })
                .collect(),
        });
        self.append_state("transcript", state);
    }

    /// Put `effect` on track `track`'s chain the way a project saved by an
    /// older build holds one: written straight into a node, not through a
    /// tool.
    pub fn plant(&mut self, track: usize, effect: EffectInstance) {
        let mut node = self.store.get(self.head()).expect("head node");
        node.state.tracks[track].effects.push(effect);
        self.store.append(node).expect("append");
    }

    /// The same, in the master chain.
    pub fn plant_master(&mut self, effect: EffectInstance) {
        let mut node = self.store.get(self.head()).expect("head node");
        node.state.master_chain.push(effect);
        self.store.append(node).expect("append");
    }

    pub fn assert_one_node_appended_after(&self, prior_head: NodeId, prior_count: usize) {
        let new_head = self.head();
        assert_ne!(new_head, prior_head, "a valid edit should move the head");
        let node = self.store.get(new_head).expect("read appended node");
        assert_eq!(
            node.parent,
            Some(prior_head),
            "edit should append to prior head"
        );
        assert_eq!(
            self.node_count(),
            prior_count + 1,
            "edit should append one node"
        );
    }

    pub fn assert_history_unchanged(
        &self,
        prior_head: NodeId,
        prior_count: usize,
        prior_state: &SessionState,
    ) {
        assert_eq!(self.head(), prior_head, "refused call moved the head");
        assert_eq!(
            self.node_count(),
            prior_count,
            "refused call appended a node"
        );
        assert_eq!(
            &self.state(),
            prior_state,
            "refused call changed session state"
        );
    }
}

/// The cancel switch and the progress sink belong to the process, as the
/// window's one Cancel button does, so two runs of a tool that reads them in
/// one test binary would press and clear each other's.
static TURN: Mutex<()> = Mutex::new(());

/// A [`Session`] that holds the process's one turn for as long as it lives,
/// with one tool swapped for another (a model tool over a fake model). It
/// derefs to the session, so `ok`, `err`, `call` and the fields are as they
/// are there. Drop it before building another on the same thread.
pub struct TurnSession {
    session: Session,
    // Fields drop in declaration order: the project is gone before the turn
    // is released to the next test.
    _turn: MutexGuard<'static, ()>,
}

impl TurnSession {
    /// An empty project whose tool `name` is `tool`.
    pub fn with_tool(name: &str, tool: Box<dyn Tool>) -> Self {
        let turn = TURN.lock().unwrap_or_else(PoisonError::into_inner);
        let mut session = Session::empty();
        assert!(session.dispatcher.unregister(name), "{name} is registered");
        session.dispatcher.register(tool);
        Self {
            session,
            _turn: turn,
        }
    }
}

impl Deref for TurnSession {
    type Target = Session;
    fn deref(&self) -> &Session {
        &self.session
    }
}

impl DerefMut for TurnSession {
    fn deref_mut(&mut self) -> &mut Session {
        &mut self.session
    }
}

/// The value of a result that must be `Ok`.
pub fn ok(r: ToolResult) -> Value {
    match r {
        ToolResult::Ok(v) => v,
        ToolResult::Error(m) => panic!("expected Ok, got Error({m})"),
    }
}

/// The value of a dispatcher answer that must be an `Ok` from the tool.
pub fn expect_ok(answer: tools::Result<ToolResult>) -> Value {
    ok(answer.expect("dispatcher invocation"))
}

/// The message of a result that must be an `Error`.
pub fn err(r: ToolResult) -> String {
    match r {
        ToolResult::Error(m) => m,
        ToolResult::Ok(v) => panic!("expected Error, got Ok({v})"),
    }
}

/// A 16-bit WAV of `frames` frames, `channels` wide, where `sample(n)` is
/// the value (-1.0 to 1.0) of frame `n` in every channel.
pub fn write_frames(
    path: &Path,
    rate: u32,
    channels: u16,
    frames: usize,
    sample: impl Fn(usize) -> f32,
) -> PathBuf {
    let spec = WavSpec {
        channels,
        sample_rate: rate,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut w = WavWriter::create(path, spec).expect("wav writer");
    for n in 0..frames {
        let v = (sample(n) * 32_767.0) as i16;
        for _ in 0..channels {
            w.write_sample(v).unwrap();
        }
    }
    w.finalize().unwrap();
    path.to_path_buf()
}

/// `seconds` of a 440 Hz sine at a quarter of full scale, mono, at `rate`.
pub fn write_tone(path: &Path, rate: u32, seconds: usize) -> PathBuf {
    write_sine(path, rate, 440.0, 0.25, rate as usize * seconds)
}

/// A mono 16-bit sine of `freq` Hz at `amp` of full scale, `frames` long.
pub fn write_sine(path: &Path, rate: u32, freq: f32, amp: f32, frames: usize) -> PathBuf {
    write_frames(path, rate, 1, frames, |n| {
        let t = n as f32 / rate as f32;
        (2.0 * std::f32::consts::PI * freq * t).sin() * amp
    })
}

/// A WAV's 16-bit samples, interleaved.
pub fn read_pcm(path: &Path) -> Vec<i16> {
    WavReader::open(path)
        .expect("open wav")
        .samples::<i16>()
        .map(|r| r.expect("sample"))
        .collect()
}

/// `seconds` of a mono sine at `hz`, 8000/32767 of full scale, at 48 kHz.
pub fn write_secs(path: &Path, seconds: f64, hz: f32) -> PathBuf {
    let frames = (48_000.0 * seconds) as usize;
    write_sine(path, 48_000, hz, 8_000.0 / 32_767.0, frames)
}

/// One second of a 440 Hz-style take: a mono sine at `freq`, a quarter of
/// full scale, at 48 kHz.
pub fn write_take(path: &Path, freq: f32) -> PathBuf {
    write_sine(path, 48_000, freq, 0.25, 48_000)
}

/// As `write_frames`, mono, but quantised the way the exactness tests need:
/// `round(sample * 32768)`, clamped. They compare whole buffers, so how the
/// source was rounded is part of what they pin.
pub fn write_exact(
    path: &Path,
    rate: u32,
    frames: usize,
    sample: impl Fn(usize) -> f32,
) -> PathBuf {
    let spec = WavSpec {
        channels: 1,
        sample_rate: rate,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut w = WavWriter::create(path, spec).expect("wav writer");
    for n in 0..frames {
        let q = (sample(n) * 32_768.0)
            .round()
            .clamp(i16::MIN as f32, i16::MAX as f32) as i16;
        w.write_sample(q).unwrap();
    }
    w.finalize().unwrap();
    path.to_path_buf()
}

/// A mono ramp from 0 to half scale, so every frame names the source frame
/// it came from and inverting cannot clip against the i16 floor.
pub fn write_half_ramp(path: &Path, rate: u32, frames: usize) -> PathBuf {
    write_exact(path, rate, frames, |n| 0.5 * n as f32 / frames as f32)
}

/// A clip of `source`: `length` frames from `source_offset`, placed at
/// `start_in_track`, with nothing else set.
pub fn clip(source: &Path, start_in_track: u64, source_offset: u64, length: u64) -> Clip {
    Clip {
        source_path: source.to_path_buf(),
        start_in_track,
        source_offset,
        length,
        content_hash: None,
        time_stretch_factor: None,
        pitch_shift_semitones: None,
        beat_grid: None,
        volume_envelope: Vec::new(),
        volume_envelope_frames: session::EnvelopeFrames::default(),
    }
}
