//! `punch_record`: replace a region of a track with a take recorded while
//! the rest of the mix plays (#203).
//!
//! The command (`commands::punch_record`) takes the gate and moves to a
//! blocking thread; everything below runs there. It is written against an
//! [`Audio`] so the tests drive the whole path (the two renders, the
//! monitor, the conversion of the take to the track's format, `punch_in`)
//! with a fake that plays the monitor back at a known delay, and the real
//! devices are `recorder::punch::record`, which the loopback test runs.
//!
//! ## What it does, in order
//!
//! 1. Names the default devices and reads the latency kept for them. With
//!    none, a measured one or a typed one, it stops here and says where to
//!    get one: no number is ever guessed.
//! 2. Reads the head: its state, the track's format (the rate its clips are
//!    counted in, its channel count), and makes the files the render needs.
//!    The store's lock is held for that and let go.
//! 3. Renders the head over `[start - pre-roll, end + post-roll)` twice, on
//!    the engine: the mix as it is, and the same state with the target
//!    track muted, which is a clone in memory and never a node. The engine's
//!    lock is held for the two renders and let go.
//! 4. Plays them to the performer (as-is outside the region, muted inside)
//!    while recording the microphone, and cuts the take out at the pair's
//!    latency (`recorder::punch`). Neither lock is held: this is the long
//!    part, and the agent, the transport and the faders go on working.
//! 5. Puts the take in the track's rate and channels (by what the
//!    microphone delivered: `recorder::punch::take_channel_map`), keeps it in
//!    the project as a WAV named by its samples (as a pasted clipboard is, so
//!    that a replay of the node finds it), and applies it with the
//!    `punch_in` tool, exactly as the agent's would, in the same hold of the
//!    store's lock that checks the head is still the one the performer
//!    heard. If it is not, the take is not applied (it would go into a mix it
//!    was not recorded against) and stays in the project, with its place
//!    named.
//!
//! The region is cut at the frames `punch_in` replaces (it truncates), for
//! the monitor, the capture and the tool alike.
//!
//! A cancel, a refusal or a failure before step 5 leaves no node and no
//! file in the project.

use std::path::PathBuf;

use recorder::duplex::DevicePair;
use recorder::punch::{self, PunchError, Region, Rendered};
use recorder::store::{LatencyStore, Source};
use recorder::RecorderError;
use serde_json::{json, Value};
use session::{NodeId, SessionState};

use crate::state::AppState;

/// The `kind` of the progress reports and of the strip that shows them.
pub const KIND: &str = "punch_record";
/// The phase after the recording, while the take is placed: Cancel is not
/// offered in it.
pub const APPLYING: &str = "applying";

pub const DEFAULT_PREROLL_SEC: f64 = 2.0;
pub const DEFAULT_POSTROLL_SEC: f64 = 1.0;
/// Longest lead-in or tail. A performer needs a bar or two, not a minute.
pub const MAX_ROLL_SEC: f64 = 30.0;
/// Longest region. The two devices drift apart by parts per million of the
/// take (about 3 ms in 30 s at 100 ppm), which is nothing for a line fix and
/// something for a verse; and the take is held in memory as it is recorded.
pub const MAX_REGION_SEC: f64 = 300.0;

/// A validated request.
#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    pub track: usize,
    pub start_sec: f64,
    pub end_sec: f64,
    pub preroll_sec: f64,
    pub postroll_sec: f64,
}

impl Request {
    /// The arguments of `punch_record`, with the defaults for the rolls.
    pub fn new(
        track: usize,
        start_sec: f64,
        end_sec: f64,
        preroll_sec: Option<f64>,
        postroll_sec: Option<f64>,
    ) -> Result<Self, String> {
        let preroll_sec = preroll_sec.unwrap_or(DEFAULT_PREROLL_SEC);
        let postroll_sec = postroll_sec.unwrap_or(DEFAULT_POSTROLL_SEC);
        if !start_sec.is_finite() || !end_sec.is_finite() || start_sec < 0.0 || end_sec <= start_sec
        {
            return Err(format!(
                "the region {start_sec} s to {end_sec} s is not a region: it has to start at 0 \
                 or later and end after it starts"
            ));
        }
        if end_sec - start_sec > MAX_REGION_SEC {
            return Err(format!(
                "a punch-in region is at most {MAX_REGION_SEC} s; this one is {:.0} s",
                end_sec - start_sec
            ));
        }
        for (what, v) in [("pre-roll", preroll_sec), ("post-roll", postroll_sec)] {
            if !v.is_finite() || !(0.0..=MAX_ROLL_SEC).contains(&v) {
                return Err(format!(
                    "the {what} is {v} s; it has to be from 0 to {MAX_ROLL_SEC} s"
                ));
            }
        }
        Ok(Self {
            track,
            start_sec,
            end_sec,
            preroll_sec,
            postroll_sec,
        })
    }
}

/// The devices, as far as a punch-in touches them.
pub trait Audio {
    fn default_pair(&self) -> Result<DevicePair, RecorderError>;
    /// Play `monitor`, record, and cut the region out
    /// ([`recorder::punch::record`]).
    fn record(
        &self,
        pair: &DevicePair,
        monitor: &[f32],
        region: Region,
        latency_sec: f64,
        cancel: &dyn Fn() -> bool,
        on_progress: &dyn Fn(punch::Progress),
    ) -> Result<punch::Take, PunchError>;
}

/// The machine's default speakers and microphone.
pub struct Devices;

impl Audio for Devices {
    fn default_pair(&self) -> Result<DevicePair, RecorderError> {
        recorder::duplex::default_pair()
    }

    fn record(
        &self,
        pair: &DevicePair,
        monitor: &[f32],
        region: Region,
        latency_sec: f64,
        cancel: &dyn Fn() -> bool,
        on_progress: &dyn Fn(punch::Progress),
    ) -> Result<punch::Take, PunchError> {
        punch::record(pair, monitor, region, latency_sec, cancel, on_progress)
    }
}

/// What the head looked like when the punch began.
struct Snapshot {
    head: NodeId,
    state: SessionState,
    project_dir: PathBuf,
    /// The rate and channel count a take for the track has to have.
    rate: u32,
    channels: u16,
}

fn lock<'a, T>(
    m: &'a std::sync::Mutex<T>,
    what: &str,
) -> Result<std::sync::MutexGuard<'a, T>, String> {
    m.lock().map_err(|_| format!("the {what} lock is poisoned"))
}

fn snapshot(state: &AppState, req: &Request) -> Result<Snapshot, String> {
    let handle = state
        .store_handle()
        .ok_or_else(|| "open a project before punching in".to_string())?;
    // Store first, engine later and apart: see the lock order in CLAUDE.md.
    let store = lock(&handle, "store")?;
    let head = store
        .head()
        .ok_or_else(|| "there is nothing in the session to punch into".to_string())?;
    let node = store.get(head).map_err(|e| e.to_string())?;
    let session = node.state;
    if req.track >= session.tracks.len() {
        return Err(format!(
            "there is no track {}: the session has {}",
            req.track,
            session.tracks.len()
        ));
    }
    let length_sec = session.length_samples as f64 / f64::from(session.sample_rate.max(1));
    if req.end_sec > length_sec + 0.01 {
        return Err(format!(
            "the region ends at {:.2} s, after the end of the session at {:.2} s. A punch-in \
             replaces audio in place and cannot make the track longer",
            req.end_sec, length_sec
        ));
    }
    // The render reads the clips' files, and so does the format: make the
    // ones history has lost first.
    tools::rederive::materialize(&store, head).map_err(|e| e.to_string())?;
    let (rate, channels) = tools::track_take_format(&session, req.track)?;
    Ok(Snapshot {
        head,
        state: session,
        project_dir: store.project_dir().to_path_buf(),
        rate,
        channels,
    })
}

/// The mix as it is and the mix without the target, over the same frames.
fn render_pair(
    state: &AppState,
    snap: &Snapshot,
    req: &Request,
    dir: &std::path::Path,
    range: audio_engine::TimeRange,
) -> Result<(Rendered, Rendered), String> {
    let as_is_path = dir.join("as-is.wav");
    let muted_path = dir.join("muted.wav");
    // A clone with the track out of the mix: muted, and not soloed, because
    // a solo overrides a mute (`graph::build` includes a soloed track
    // whatever its mute) and a soloed target would still be heard inside the
    // region. It is never stored, so no node is made.
    let mut muted = snap.state.clone();
    muted.tracks[req.track].muted = true;
    muted.tracks[req.track].soloed = false;
    let could_not =
        |e: audio_engine::Error| format!("could not render the mix for the monitor: {e}");
    let muted_render = {
        let engine = lock(&state.engine, "engine")?;
        engine
            .render_to_wav(&snap.state, &as_is_path, Some(range))
            .map_err(could_not)?;
        engine.render_to_wav(&muted, &muted_path, Some(range))
    };
    let read = |p: &PathBuf| punch::read_wav(p).map_err(|e| e.to_string());
    let as_is = read(&as_is_path)?;
    let muted = match muted_render {
        Ok(_) => read(&muted_path)?,
        // The renderer sizes a render from the tracks that contribute, and
        // refuses a range that starts after they end. With the target out
        // of the mix there may be nothing left that reaches the region (a
        // session of one track; a vocal that outlasts the bed), and then
        // what is heard inside it is silence, which the monitor pads.
        Err(audio_engine::Error::InvalidRange) => Rendered {
            samples: Vec::new(),
            sample_rate: as_is.sample_rate,
            channels: as_is.channels,
        },
        Err(e) => return Err(could_not(e)),
    };
    Ok((as_is, muted))
}

/// What `run` ends with besides an error.
fn cancelled() -> Value {
    json!({ "cancelled": true })
}

/// The frames of the track's own buffer that `punch_in` replaces for a
/// region in seconds: it truncates both ends to a frame
/// (`punch_in::apply_punch`), so the take, the monitor's region and the
/// recording all have to be cut at these and not at a rounding of their
/// own, or they differ by a frame at an edge.
fn region_frames(start_sec: f64, end_sec: f64, rate: u32) -> (u64, u64) {
    let r = f64::from(rate);
    (
        (start_sec * r).max(0.0) as u64,
        (end_sec * r).max(0.0) as u64,
    )
}

/// Punch in. See the module docs. `cancel` is polled while the take is
/// recorded and between the steps; `report` gets the progress events.
pub fn run(
    state: &AppState,
    audio: &dyn Audio,
    store: &LatencyStore,
    req: &Request,
    cancel: &dyn Fn() -> bool,
    report: &dyn Fn(Value),
) -> Result<Value, String> {
    let pair = audio.default_pair().map_err(|e| e.to_string())?;
    let stored = punch::latency_for(store, &pair).map_err(|e| e.to_string())?;
    let latency_sec = stored.latency_ms / 1_000.0;

    // The first report: from here the window has a strip for this job.
    report(json!({ "kind": KIND, "phase": "preparing" }));
    let snap = snapshot(state, req)?;
    if cancel() {
        return Ok(cancelled());
    }

    // The region in the track's own frames, as `punch_in` will cut it.
    let (f0, f1) = region_frames(req.start_sec, req.end_sec, snap.rate);
    if f1 <= f0 {
        return Err("that region is less than one sample long".to_string());
    }
    let rate = f64::from(snap.rate);
    let (start_sec, end_sec) = (f0 as f64 / rate, f1 as f64 / rate);

    // Whole frames, so the region's place in the render is exact.
    let sr = f64::from(snap.state.sample_rate.max(1));
    let start_frame = ((start_sec - req.preroll_sec).max(0.0) * sr).floor() as u64;
    let end_frame = ((end_sec + req.postroll_sec) * sr).ceil() as u64;
    let begins = start_frame as f64 / sr;
    let region = Region {
        start_sec: start_sec - begins,
        end_sec: end_sec - begins,
    };

    let dir = tempfile::Builder::new()
        .prefix("edytlab-punch-")
        .tempdir()
        .map_err(|e| format!("could not make a folder for the renders: {e}"))?;
    let (as_is, muted) = render_pair(
        state,
        &snap,
        req,
        dir.path(),
        audio_engine::TimeRange {
            start_frame,
            end_frame,
        },
    )?;
    let monitor = punch::build_monitor(&as_is, &muted, region, pair.out_sr, pair.out_ch)
        .map_err(|e| e.to_string())?;
    drop((as_is, muted));
    drop(dir);
    if cancel() {
        return Ok(cancelled());
    }

    let take = match audio.record(&pair, &monitor, region, latency_sec, cancel, &|p| {
        report(json!({
            "kind": KIND,
            "phase": p.phase.as_str(),
            "remaining_sec": p.remaining_sec,
        }));
    }) {
        Ok(t) => t,
        Err(PunchError::Cancelled) => return Ok(cancelled()),
        Err(e) => return Err(e.to_string()),
    };
    drop(monitor);
    // The take is over: from here Cancel is no longer offered.
    report(json!({ "kind": KIND, "phase": APPLYING }));

    // The take, in the track's rate and channels and exactly the region
    // long (a frame of rounding either way is trimmed or silent).
    let mut samples = punch::convert_take(
        &take.samples,
        take.sample_rate,
        take.channels as usize,
        snap.rate,
        snap.channels as usize,
    )
    .map_err(|e| e.to_string())?;
    samples.resize((f1 - f0) as usize * snap.channels as usize, 0.0);
    if cancel() {
        return Ok(cancelled());
    }

    // The take is kept in the project, named by its samples, as a pasted
    // clipboard is: the node records `punch_in` with this path, and a
    // replay of it (a rebuild after the history sweep) has to find the take
    // where it was, which a temporary file would not be.
    let blob = tools::provenance::store_clipboard_blob(
        &snap.project_dir,
        &samples,
        snap.rate,
        snap.channels,
    )?;
    let take_path = tools::provenance::clipboard_blob_path(&snap.project_dir, &blob);
    // From here the take is the only copy of a performance: whatever goes
    // wrong, it stays in the project and its place is named.
    let keep =
        |why: String| -> String { format!("{why} The take is saved in {}.", take_path.display()) };

    // Compare and apply in one hold of the store's lock: an edit that lands
    // between a check and the tool would put the take on a mix it was not
    // recorded against. `punch_in` cuts at a frame by truncating, so the
    // times given are the middle of the frames meant.
    let head = match crate::commands::run_track_tool_at(
        state,
        "punch_in",
        json!({
            "track": req.track,
            "start_sec": (f0 as f64 + 0.5) / rate,
            "end_sec": (f1 as f64 + 0.5) / rate,
            "take_path": take_path,
        }),
        Some(snap.head),
    ) {
        Ok(h) => h,
        Err(crate::commands::ApplyError::HeadMoved) => {
            return Err(keep(
                "The project changed while the take was being recorded, so it was not \
                 applied: it was recorded against a mix that is no longer the current one."
                    .into(),
            ))
        }
        Err(crate::commands::ApplyError::Failed(e)) => {
            return Err(keep(format!("The take could not be applied: {e}.")))
        }
    };

    Ok(json!({
        "cancelled": false,
        "head": head,
        "latency_ms": stored.latency_ms,
        "latency_source": match stored.source {
            Source::Measured => "measured",
            Source::Manual => "manual",
        },
        "start_sec": start_sec,
        "end_sec": end_sec,
        "track": req.track,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    use recorder::duplex::Capture;
    use recorder::store::Stored;
    use serde_json::json;

    /// The rate of the files in the session, and of the track being punched.
    const SESSION_SR: u32 = 48_000;
    /// What the fake devices are: a 44.1 kHz stereo output and a 48 kHz
    /// mono microphone, so the monitor and the take both change rate and
    /// channels on the way.
    const OUT_SR: u32 = 44_100;
    const IN_SR: u32 = 48_000;
    /// How the fake room delays things: the output starts 30 ms after the
    /// microphone, and the sound takes 45 ms to come back.
    const SKEW_NS: i64 = 30_000_000;
    const TRUE_LATENCY_MS: f64 = 45.0;

    fn pair() -> DevicePair {
        DevicePair {
            host: "Fake".into(),
            output: "Speakers".into(),
            input: "Mic".into(),
            out_sr: OUT_SR,
            out_ch: 2,
            in_sr: IN_SR,
            in_ch: 1,
        }
    }

    /// Roughly Gaussian noise from a seeded generator.
    fn noise(frames: usize, ch: usize, seed: u64, level: f32) -> Vec<f32> {
        let mut x = seed | 1;
        let mut out = Vec::with_capacity(frames * ch);
        for _ in 0..frames {
            let mut v = 0.0f32;
            for _ in 0..4 {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                v += (x >> 11) as f32 / (1u64 << 53) as f32 - 0.5;
            }
            out.extend(std::iter::repeat_n(v * level, ch));
        }
        out
    }

    fn rms(x: &[f32]) -> f64 {
        (x.iter().map(|v| f64::from(*v) * f64::from(*v)).sum::<f64>() / x.len().max(1) as f64)
            .sqrt()
    }

    /// Normalised correlation of two equal-length buffers.
    fn similarity(a: &[f32], b: &[f32]) -> f64 {
        let n = a.len().min(b.len());
        let dot: f64 = (0..n).map(|i| f64::from(a[i]) * f64::from(b[i])).sum();
        dot / (rms(&a[..n]) * rms(&b[..n]) * n as f64).max(1e-12)
    }

    /// What the fake microphone is told to do, and what it was given.
    struct Fake {
        /// `None`: the room answers the way it should.
        outcome: RefCell<Option<Result<(), PunchError>>>,
        calls: Cell<usize>,
        heard: RefCell<Vec<f32>>,
        latency_sec: Cell<f64>,
        phases: RefCell<Vec<String>>,
        /// Run once the monitor has been played, before the take is returned:
        /// the user doing something while the performer is playing.
        meanwhile: RefCell<Option<Box<dyn FnOnce()>>>,
    }

    impl Fake {
        fn new() -> Self {
            Self {
                outcome: RefCell::new(None),
                calls: Cell::new(0),
                heard: RefCell::new(Vec::new()),
                latency_sec: Cell::new(0.0),
                phases: RefCell::new(Vec::new()),
                meanwhile: RefCell::new(None),
            }
        }
    }

    impl Audio for Fake {
        fn default_pair(&self) -> Result<DevicePair, RecorderError> {
            Ok(pair())
        }

        fn record(
            &self,
            pair: &DevicePair,
            monitor: &[f32],
            region: Region,
            latency_sec: f64,
            _cancel: &dyn Fn() -> bool,
            on_progress: &dyn Fn(punch::Progress),
        ) -> Result<punch::Take, PunchError> {
            self.calls.set(self.calls.get() + 1);
            self.latency_sec.set(latency_sec);
            *self.heard.borrow_mut() = monitor.to_vec();
            on_progress(punch::progress_at(0.0, region, 1.0));
            self.phases.borrow_mut().push("preroll".into());
            if let Some(f) = self.meanwhile.borrow_mut().take() {
                f();
            }
            if let Some(Err(e)) = self.outcome.borrow_mut().take() {
                return Err(e);
            }
            // The performer plays exactly what they hear, and the room
            // brings it back TRUE_LATENCY_MS after the output started
            // SKEW_NS after the microphone did.
            let mono = punch::convert(monitor, pair.out_sr, 2, pair.in_sr, 1).unwrap();
            let lead = ((SKEW_NS as f64 * 1e-9 + TRUE_LATENCY_MS / 1e3) * f64::from(pair.in_sr))
                .round() as usize;
            let total = punch::capture_frames(
                (monitor.len() / 2) as f64 / f64::from(pair.out_sr),
                region,
                latency_sec,
                pair.in_sr,
            );
            let mut samples = vec![0.0f32; lead];
            samples.extend_from_slice(&mono);
            samples.resize(total, 0.0);
            let capture = Capture {
                samples,
                in_sr: pair.in_sr,
                in_ch: 1,
                out_origin_ns: Some(1_000_000 + SKEW_NS),
                in_origin_ns: Some(1_000_000),
                xrun: false,
                cancelled: false,
            };
            punch::extract(&capture, region, latency_sec)
        }
    }

    fn store_with(ms: Option<(f64, Source)>) -> (tempfile::TempDir, LatencyStore) {
        let dir = tempfile::tempdir().unwrap();
        let s = LatencyStore::at(dir.path().join("audio-latency.json"));
        match ms {
            Some((ms, Source::Manual)) => {
                s.put_manual(&pair(), ms).unwrap();
            }
            Some((ms, Source::Measured)) => s
                .put(
                    &pair(),
                    Stored {
                        latency_ms: ms,
                        spread_ms: Some(0.1),
                        measured_at: 1,
                        source: Source::Measured,
                    },
                )
                .unwrap(),
            None => {}
        }
        (dir, s)
    }

    /// A project with two tracks over four seconds: track 0 is the take
    /// being replaced (loud noise, stereo), track 1 is the accompaniment
    /// (quiet noise, mono), both at 48 kHz.
    struct Project {
        _dir: tempfile::TempDir,
        state: AppState,
        /// Track 0 as it was loaded.
        original: Vec<f32>,
        accompaniment: Vec<f32>,
    }

    fn project() -> Project {
        project_with(4.0, Some(4.0))
    }

    /// The lead track `lead_sec` long, and a bed under it `band_sec` long
    /// (`None`: no bed, a session of one track).
    fn project_with(lead_sec: f64, band_sec: Option<f64>) -> Project {
        let dir = tempfile::tempdir().unwrap();
        let a = noise((SESSION_SR as f64 * lead_sec) as usize, 2, 0xA1, 0.4);
        let b = match band_sec {
            Some(sec) => noise((SESSION_SR as f64 * sec) as usize, 1, 0xB2, 0.1),
            None => Vec::new(),
        };
        let (pa, pb) = (dir.path().join("lead.wav"), dir.path().join("band.wav"));
        punch::write_wav(&pa, &a, SESSION_SR, 2).unwrap();
        punch::write_wav(&pb, &b, SESSION_SR, 1).unwrap();
        let proj = dir.path().join("project");
        std::fs::create_dir_all(&proj).unwrap();
        {
            let mut store = session::Store::open(&proj).unwrap();
            let mut engine = audio_engine::Engine::new();
            let mut clipboard = None;
            let dispatcher = tools::ToolDispatcher::default_dispatcher();
            let paths: Vec<_> = if band_sec.is_some() {
                vec![&pa, &pb]
            } else {
                vec![&pa]
            };
            for path in paths {
                let mut ctx = tools::ToolContext {
                    store: &mut store,
                    engine: &mut engine,
                    user_message: "",
                    clipboard: &mut clipboard,
                    allowed_tools: None,
                };
                match dispatcher
                    .invoke("load", json!({ "path": path }), &mut ctx)
                    .unwrap()
                {
                    tools::ToolResult::Ok(_) => {}
                    tools::ToolResult::Error(m) => panic!("load failed: {m}"),
                }
            }
        }
        let state = AppState::new();
        crate::commands::open_project_for_test(&state, &proj).unwrap();
        Project {
            _dir: dir,
            state,
            original: a,
            accompaniment: b,
        }
    }

    impl Project {
        fn head(&self) -> NodeId {
            self.state
                .store_handle()
                .unwrap()
                .lock()
                .unwrap()
                .head()
                .unwrap()
        }

        fn nodes(&self) -> usize {
            self.state
                .store_handle()
                .unwrap()
                .lock()
                .unwrap()
                .list_nodes()
                .unwrap()
                .len()
        }

        /// Track `n`'s audio at the head, as the file it plays from holds it.
        fn track_audio(&self, n: usize) -> Rendered {
            let handle = self.state.store_handle().unwrap();
            let store = handle.lock().unwrap();
            let s = store.get(store.head().unwrap()).unwrap().state;
            punch::read_wav(&s.tracks[n].clips[0].source_path).unwrap()
        }
    }

    fn req() -> Request {
        Request::new(0, 1.0, 2.0, Some(0.5), Some(0.25)).unwrap()
    }

    fn run_with(
        p: &Project,
        fake: &Fake,
        store: &LatencyStore,
        r: &Request,
    ) -> Result<Value, String> {
        run(&p.state, fake, store, r, &|| false, &|v| {
            assert_eq!(v["kind"], KIND);
        })
    }

    #[test]
    fn a_take_is_the_accompaniment_over_the_region_and_the_rest_of_the_track_is_untouched() {
        let p = project();
        let (_d, store) = store_with(Some((TRUE_LATENCY_MS, Source::Measured)));
        let before = p.head();
        let nodes = p.nodes();
        let fake = Fake::new();

        let out = run_with(&p, &fake, &store, &req()).expect("a punch");

        assert_eq!(out["cancelled"], false);
        assert_eq!(out["latency_ms"], TRUE_LATENCY_MS);
        assert_eq!(out["latency_source"], "measured");
        let head = out["head"].as_str().unwrap();
        assert_eq!(head, p.head().to_hex(), "the answer is the head");
        assert_ne!(p.head(), before);
        assert_eq!(p.nodes(), nodes + 1, "exactly one node: the punch");
        assert!((fake.latency_sec.get() - 0.045).abs() < 1e-12);

        // Track 0 now holds what the performer played in the region ...
        let now = p.track_audio(0);
        assert_eq!((now.sample_rate, now.channels), (SESSION_SR, 2));
        let (s, e) = (SESSION_SR as usize, SESSION_SR as usize * 2);
        let region: Vec<f32> = now.samples[s * 2..e * 2].to_vec();
        // ... which is the accompaniment (what was left unmuted), twice for
        // the two channels, as loud as the pan law leaves it.
        let band: Vec<f32> = p.accompaniment[s..e]
            .iter()
            .flat_map(|v| [*v, *v])
            .collect();
        let sim = similarity(&region, &band);
        // Two passes through a resampler (48 to 44.1 and back) take the top
        // of white noise's band off, so it is close, not identical.
        assert!(sim > 0.9, "the take is not the accompaniment: {sim}");
        // And it is on the beat to the frame: shifted by one it would be gone.
        for shift in [-48i64, -1, 1, 48] {
            let off: Vec<f32> = p.accompaniment
                [(s as i64 + shift) as usize..(e as i64 + shift) as usize]
                .iter()
                .flat_map(|v| [*v, *v])
                .collect();
            assert!(
                similarity(&region, &off) < 0.3,
                "shifted by {shift} frames it still matches"
            );
        }
        // Outside the region the track is what it was (the edit keeps 16-bit
        // PCM, so to within one step of it).
        let worst = |a: &[f32], b: &[f32]| {
            a.iter()
                .zip(b)
                .map(|(x, y)| (x - y).abs())
                .fold(0.0f32, f32::max)
        };
        assert!(worst(&now.samples[..s * 2], &p.original[..s * 2]) < 1e-4);
        assert!(worst(&now.samples[e * 2..], &p.original[e * 2..]) < 1e-4);
        assert_eq!(
            now.samples.len(),
            p.original.len(),
            "the length is unchanged"
        );
    }

    #[test]
    fn the_performer_hears_the_whole_mix_but_not_the_track_inside_the_region() {
        let p = project();
        let (_d, store) = store_with(Some((TRUE_LATENCY_MS, Source::Measured)));
        let fake = Fake::new();
        run_with(&p, &fake, &store, &req()).unwrap();

        // The render began half a second before the region (the pre-roll).
        let m = fake.heard.borrow();
        let at = |sec: f64| (sec * f64::from(OUT_SR)) as usize * 2;
        let pre = rms(&m[at(0.05)..at(0.45)]);
        let inside = rms(&m[at(0.6)..at(1.4)]);
        let post = rms(&m[at(1.55)..at(1.7)]);
        assert!(pre > 0.2, "the lead-in has the track in it: {pre}");
        assert!(post > 0.2, "and so does the tail: {post}");
        assert!(
            inside < 0.1,
            "inside the region only the accompaniment plays: {inside}"
        );
        assert!(inside > 0.02, "and it does play: {inside}");
        // The monitor is in the speakers' format, and as long as the render.
        assert_eq!(m.len() % 2, 0);
        let sec = (m.len() / 2) as f64 / f64::from(OUT_SR);
        assert!((sec - 1.75).abs() < 0.01, "{sec} s for 0.5 + 1 + 0.25");
        // The progress strip was told.
        assert_eq!(fake.phases.borrow().as_slice(), ["preroll"]);
    }

    #[test]
    fn a_typed_latency_counts_exactly_as_a_measured_one() {
        let p = project();
        let (_d, store) = store_with(Some((TRUE_LATENCY_MS, Source::Manual)));
        let out = run_with(&p, &Fake::new(), &store, &req()).expect("a punch");
        assert_eq!(out["latency_source"], "manual");
        assert_eq!(out["latency_ms"], TRUE_LATENCY_MS);
    }

    #[test]
    fn a_punch_with_no_latency_kept_is_refused_before_anything_is_rendered_or_played() {
        let p = project();
        let (_d, store) = store_with(None);
        let before = p.head();
        let nodes = p.nodes();
        let fake = Fake::new();

        let e = run_with(&p, &fake, &store, &req()).unwrap_err();

        assert!(e.contains("Settings") && e.contains("Audio"), "{e}");
        assert!(e.contains("Measure") && e.contains("type the delay"), "{e}");
        assert_eq!(fake.calls.get(), 0, "nothing was played");
        assert_eq!(p.head(), before);
        assert_eq!(p.nodes(), nodes);
    }

    #[test]
    fn a_cancelled_punch_leaves_no_node_and_no_file() {
        let p = project();
        let (_d, store) = store_with(Some((TRUE_LATENCY_MS, Source::Measured)));
        let before = p.head();
        let nodes = p.nodes();
        let fake = Fake::new();
        *fake.outcome.borrow_mut() = Some(Err(PunchError::Cancelled));

        let out = run_with(&p, &fake, &store, &req()).expect("cancel is not an error");

        assert_eq!(out, json!({ "cancelled": true }));
        assert_eq!(p.head(), before);
        assert_eq!(p.nodes(), nodes);
    }

    #[test]
    fn a_cancel_before_the_monitor_starts_plays_nothing() {
        let p = project();
        let (_d, store) = store_with(Some((TRUE_LATENCY_MS, Source::Measured)));
        let fake = Fake::new();
        let out = run(&p.state, &fake, &store, &req(), &|| true, &|_| {}).unwrap();
        assert_eq!(out, json!({ "cancelled": true }));
        assert_eq!(fake.calls.get(), 0);
    }

    #[test]
    fn a_take_the_device_lost_frames_of_is_refused_and_leaves_no_node() {
        let p = project();
        let (_d, store) = store_with(Some((TRUE_LATENCY_MS, Source::Measured)));
        let nodes = p.nodes();
        let fake = Fake::new();
        *fake.outcome.borrow_mut() = Some(Err(PunchError::Dropout));
        let e = run_with(&p, &fake, &store, &req()).unwrap_err();
        assert!(e.contains("dropped out"), "{e}");
        assert_eq!(p.nodes(), nodes);
    }

    #[test]
    fn a_take_recorded_against_a_mix_that_has_since_changed_is_kept_not_applied() {
        let p = project();
        let (_d, store) = store_with(Some((TRUE_LATENCY_MS, Source::Measured)));
        let fake = Fake::new();
        let state = p.state.clone();
        // While the performer plays, the user nudges a fader.
        *fake.meanwhile.borrow_mut() = Some(Box::new(move || {
            crate::commands::run_track_tool(
                &state,
                "set_track_gain",
                json!({ "track": 1, "db": -3.0 }),
            )
            .unwrap();
        }));
        let nodes = p.nodes();

        let e = run_with(&p, &fake, &store, &req()).unwrap_err();

        assert!(e.contains("changed while"), "{e}");
        assert_eq!(p.nodes(), nodes + 1, "only the fader's node, no punch");
        // The take is in the project, where the message says.
        let kept = e.rsplit("saved in ").next().unwrap().trim_end_matches('.');
        let kept = std::path::PathBuf::from(kept);
        assert!(kept.exists(), "{e}");
        assert!(kept.to_string_lossy().contains(".audiograph"), "{e}");
        let take = punch::read_wav(&kept).unwrap();
        assert_eq!((take.sample_rate, take.channels), (SESSION_SR, 2));
        assert_eq!(take.samples.len(), SESSION_SR as usize * 2, "one second");
    }

    #[test]
    fn a_region_the_session_does_not_reach_or_a_missing_track_is_refused() {
        let p = project();
        let (_d, store) = store_with(Some((TRUE_LATENCY_MS, Source::Measured)));
        let fake = Fake::new();
        let late = Request::new(0, 3.5, 4.5, None, None).unwrap();
        let e = run_with(&p, &fake, &store, &late).unwrap_err();
        assert!(e.contains("after the end of the session"), "{e}");
        let nowhere = Request::new(7, 1.0, 2.0, None, None).unwrap();
        let e = run_with(&p, &fake, &store, &nowhere).unwrap_err();
        assert!(e.contains("no track 7"), "{e}");
        assert_eq!(fake.calls.get(), 0);
    }

    #[test]
    fn a_punch_into_a_session_with_no_project_open_says_so() {
        let state = AppState::new();
        let (_d, store) = store_with(Some((TRUE_LATENCY_MS, Source::Measured)));
        let e = run(&state, &Fake::new(), &store, &req(), &|| false, &|_| {}).unwrap_err();
        assert!(e.contains("open a project"), "{e}");
    }

    #[test]
    fn the_request_is_checked_and_given_its_defaults() {
        let r = Request::new(1, 2.0, 3.0, None, None).unwrap();
        assert_eq!((r.preroll_sec, r.postroll_sec), (2.0, 1.0));
        assert!(Request::new(0, 3.0, 2.0, None, None).is_err());
        assert!(Request::new(0, 2.0, 2.0, None, None).is_err());
        assert!(Request::new(0, -1.0, 2.0, None, None).is_err());
        assert!(Request::new(0, f64::NAN, 2.0, None, None).is_err());
        assert!(Request::new(0, 0.0, 301.0, None, None).is_err());
        assert!(Request::new(0, 0.0, 300.0, None, None).is_ok());
        assert!(Request::new(0, 1.0, 2.0, Some(-1.0), None).is_err());
        assert!(Request::new(0, 1.0, 2.0, None, Some(31.0)).is_err());
        assert!(Request::new(0, 1.0, 2.0, Some(0.0), Some(0.0)).is_ok());
        assert!(Request::new(0, 1.0, 2.0, Some(f64::INFINITY), None).is_err());
    }

    fn monitor_levels(fake: &Fake) -> (f64, f64, f64) {
        let m = fake.heard.borrow();
        let at = |sec: f64| (sec * f64::from(OUT_SR)) as usize * 2;
        (
            rms(&m[at(0.05)..at(0.45)]),
            rms(&m[at(0.6)..at(1.4)]),
            rms(&m[at(1.55)..at(1.7)]),
        )
    }

    #[test]
    fn a_session_of_one_track_can_be_punched_the_rest_of_the_mix_is_silence() {
        // Muted, the only track leaves nothing: the renderer refuses a range
        // that starts after the (empty) mix ends. That is silence, not a
        // failure.
        let p = project_with(4.0, None);
        let (_d, store) = store_with(Some((TRUE_LATENCY_MS, Source::Measured)));
        let nodes = p.nodes();
        let fake = Fake::new();

        let out = run_with(&p, &fake, &store, &req()).expect("a punch into a single track");

        assert_eq!(out["cancelled"], false);
        assert_eq!(p.nodes(), nodes + 1);
        let (pre, inside, post) = monitor_levels(&fake);
        assert!(
            pre > 0.2 && post > 0.2,
            "the track leads in and out: {pre} {post}"
        );
        assert!(inside < 1e-3, "and nothing else plays inside: {inside}");
        // The performer played silence: the region is silent, the rest is not.
        let now = p.track_audio(0);
        let (s, e) = (SESSION_SR as usize, SESSION_SR as usize * 2);
        // (the first and last 10 ms hold the monitor's 5 ms fades)
        let edge = 480 * 2;
        assert!(rms(&now.samples[s * 2 + edge..e * 2 - edge]) < 1e-3);
        assert!(rms(&now.samples[..s * 2]) > 0.2);
        assert!(rms(&now.samples[e * 2..]) > 0.2);
    }

    #[test]
    fn a_target_that_outlasts_everything_else_can_be_punched_where_only_it_plays() {
        // A 4 s lead over a 1 s bed, punched at 2-3 s: the muted mix is over
        // before the pre-roll even starts.
        let p = project_with(4.0, Some(1.0));
        let (_d, store) = store_with(Some((TRUE_LATENCY_MS, Source::Measured)));
        let nodes = p.nodes();
        let fake = Fake::new();
        let r = Request::new(0, 2.0, 3.0, Some(0.5), Some(0.25)).unwrap();

        let out = run_with(&p, &fake, &store, &r).expect("a punch past the bed");

        assert_eq!(out["cancelled"], false);
        assert_eq!(p.nodes(), nodes + 1);
        let (_, inside, _) = monitor_levels(&fake);
        assert!(inside < 1e-3, "the bed is over: {inside}");
    }

    #[test]
    fn a_bed_that_ends_inside_the_region_is_heard_to_its_end_and_then_silence() {
        // The bed lasts to 1.75 s of a region at 1-2 s: the muted render is
        // shorter than the monitor and is padded.
        let p = project_with(4.0, Some(1.75));
        let (_d, store) = store_with(Some((TRUE_LATENCY_MS, Source::Measured)));
        let fake = Fake::new();
        run_with(&p, &fake, &store, &req()).expect("a punch");
        let m = fake.heard.borrow();
        let at = |sec: f64| (sec * f64::from(OUT_SR)) as usize * 2;
        // The monitor began at 0.5 s of the session.
        let early = rms(&m[at(0.6)..at(1.0)]); // session 1.1 to 1.5
        let late = rms(&m[at(1.4)..at(1.49)]); // session 1.9 to 1.99 (the last 10 ms is the fade)
        assert!(early > 0.02, "the bed plays: {early}");
        assert!(late < 1e-3, "and is over: {late}");
    }

    #[test]
    fn a_soloed_target_is_still_out_of_the_mix_inside_the_region() {
        // A solo overrides a mute in the renderer, so a clone that only
        // muted the target would still play it there.
        let p = project();
        crate::commands::run_track_tool(
            &p.state,
            "solo_track",
            json!({ "track": 1, "solo": true }),
        )
        .unwrap();
        crate::commands::run_track_tool(
            &p.state,
            "solo_track",
            json!({ "track": 1, "solo": false }),
        )
        .unwrap();
        crate::commands::run_track_tool(
            &p.state,
            "solo_track",
            json!({ "track": 0, "solo": true }),
        )
        .unwrap();
        let (_d, store) = store_with(Some((TRUE_LATENCY_MS, Source::Measured)));
        let fake = Fake::new();
        run_with(&p, &fake, &store, &req()).expect("a punch");

        let (pre, inside, post) = monitor_levels(&fake);
        // Outside the region the user's own solo is what they hear: the target.
        assert!(pre > 0.2 && post > 0.2, "{pre} {post}");
        // Inside, the target is out and the rest of the mix plays.
        assert!(
            inside < 0.1,
            "the soloed target is heard in the region: {inside}"
        );
        assert!(inside > 0.02, "and the rest is not silent: {inside}");
    }

    #[test]
    fn an_edit_is_made_only_on_the_head_the_caller_names_and_otherwise_changes_nothing() {
        let p = project();
        let before = p.head();
        let nodes = p.nodes();
        crate::commands::run_track_tool(
            &p.state,
            "set_track_gain",
            json!({ "track": 1, "db": -3.0 }),
        )
        .unwrap();
        assert_eq!(p.nodes(), nodes + 1);

        // Named the head it was prepared against, after the head moved.
        let e = crate::commands::run_track_tool_at(
            &p.state,
            "set_track_gain",
            json!({ "track": 1, "db": -9.0 }),
            Some(before),
        )
        .unwrap_err();
        assert_eq!(e, crate::commands::ApplyError::HeadMoved);
        assert_eq!(p.nodes(), nodes + 1, "no node was made");

        // Named the head it is on.
        let now = p.head();
        let head = crate::commands::run_track_tool_at(
            &p.state,
            "set_track_gain",
            json!({ "track": 1, "db": -9.0 }),
            Some(now),
        )
        .unwrap();
        assert_eq!(head, p.head().to_hex());
        assert_eq!(p.nodes(), nodes + 2);
    }

    #[test]
    fn a_head_that_moves_after_the_take_is_cut_never_gets_the_punch() {
        // The window's edit lands after the take was recorded and before
        // the tool runs: here, the last thing the fake does. The tool is
        // asked to run on the head the performer heard, and refuses.
        let p = project();
        let (_d, store) = store_with(Some((TRUE_LATENCY_MS, Source::Measured)));
        let fake = Fake::new();
        let state = p.state.clone();
        *fake.meanwhile.borrow_mut() = Some(Box::new(move || {
            crate::commands::run_track_tool(
                &state,
                "set_track_gain",
                json!({ "track": 0, "db": -1.0 }),
            )
            .unwrap();
        }));
        let before = p.track_audio(0).samples;
        let e = run_with(&p, &fake, &store, &req()).unwrap_err();
        assert!(e.contains("changed while"), "{e}");
        assert_eq!(
            p.track_audio(0).samples,
            before,
            "the audio was not touched"
        );
    }

    #[test]
    fn the_punched_node_can_be_replayed_because_its_take_is_in_the_project() {
        let p = project();
        let (_d, store) = store_with(Some((TRUE_LATENCY_MS, Source::Measured)));
        run_with(&p, &Fake::new(), &store, &req()).unwrap();

        let handle = p.state.store_handle().unwrap();
        let st = handle.lock().unwrap();
        let head = st.head().unwrap();
        let node = st.get(head).unwrap();
        let op = node.op.expect("the punch recorded its op");
        assert_eq!(op.tool, "punch_in");
        assert!(op.reproducible);
        let take = std::path::PathBuf::from(op.params["take_path"].as_str().unwrap());
        assert!(take.is_file(), "the op names a take that exists");
        assert!(
            take.starts_with(st.project_dir()),
            "and it is in the project, not a temporary folder: {}",
            take.display()
        );
        // And a replay of the step on its parent rebuilds the punch.
        let replay = tools::rederive::replay(&st, head).expect("replays");
        assert!(
            !replay.produced().is_empty(),
            "it wrote the punched audio again"
        );
    }

    #[test]
    fn the_strip_is_told_the_take_is_being_placed_once_the_recording_is_over() {
        let p = project();
        let (_d, store) = store_with(Some((TRUE_LATENCY_MS, Source::Measured)));
        let seen = RefCell::new(Vec::new());
        run(&p.state, &Fake::new(), &store, &req(), &|| false, &|v| {
            seen.borrow_mut()
                .push(v["phase"].as_str().unwrap_or("").to_string());
        })
        .unwrap();
        let seen = seen.into_inner();
        assert_eq!(seen.first().map(String::as_str), Some("preparing"));
        assert_eq!(seen.last().map(String::as_str), Some(APPLYING));
    }

    #[test]
    fn a_cancel_pressed_after_the_take_ended_and_before_it_is_applied_applies_nothing() {
        let p = project();
        let (_d, store) = store_with(Some((TRUE_LATENCY_MS, Source::Measured)));
        let nodes = p.nodes();
        let polls = Cell::new(0);
        // Not cancelled before or during the recording (the fake never
        // polls); cancelled the third time it is asked: the one after the
        // take is cut.
        let out = run(
            &p.state,
            &Fake::new(),
            &store,
            &req(),
            &|| {
                polls.set(polls.get() + 1);
                polls.get() >= 3
            },
            &|_| {},
        )
        .unwrap();
        assert_eq!(out, json!({ "cancelled": true }));
        assert_eq!(p.nodes(), nodes);
    }

    #[test]
    fn a_region_off_the_frame_grid_is_cut_at_the_frames_the_tool_replaces() {
        // 1.000013 s to 2.000029 s at 48 kHz is frames 48 000.6 to 96 001.4:
        // the tool truncates both, so frames 48 000 to 96 001.
        let p = project();
        let (_d, store) = store_with(Some((TRUE_LATENCY_MS, Source::Measured)));
        let r = Request::new(0, 1.000_013, 2.000_029, Some(0.5), Some(0.25)).unwrap();
        let out = run_with(&p, &Fake::new(), &store, &r).expect("a punch");
        assert_eq!(out["start_sec"].as_f64().unwrap(), 48_000.0 / 48_000.0);
        assert_eq!(out["end_sec"].as_f64().unwrap(), 96_001.0 / 48_000.0);

        let now = p.track_audio(0);
        let (s, e) = (48_000usize, 96_001usize);
        let region: Vec<f32> = now.samples[s * 2..e * 2].to_vec();
        let band: Vec<f32> = p.accompaniment[s..e]
            .iter()
            .flat_map(|v| [*v, *v])
            .collect();
        assert!(similarity(&region, &band) > 0.9);
        // The frame before and the frame after are the original's.
        assert!((now.samples[(s - 1) * 2] - p.original[(s - 1) * 2]).abs() < 1e-4);
        assert!((now.samples[e * 2] - p.original[e * 2]).abs() < 1e-4);
        // The edges agree to the frame: shifted by one it would not match.
        let off: Vec<f32> = p.accompaniment[s + 1..e + 1]
            .iter()
            .flat_map(|v| [*v, *v])
            .collect();
        assert!(similarity(&region, &off) < 0.3);
    }

    #[test]
    fn the_region_frames_are_the_tools_truncation() {
        assert_eq!(region_frames(1.0, 2.0, 48_000), (48_000, 96_000));
        assert_eq!(
            region_frames(1.000_013, 2.000_029, 48_000),
            (48_000, 96_001)
        );
        assert_eq!(
            region_frames(0.5, 0.5 + 1.0 / 44_100.0, 44_100),
            (22_050, 22_051)
        );
    }

    #[test]
    fn once_history_moves_on_the_punched_audio_is_swept_and_comes_back_byte_for_byte() {
        // The proof that the node is replayable: the history sweep deletes
        // audio only older nodes name, and only once a replay has shown it
        // can be rebuilt. With the take in a temporary folder that replay
        // fails and the sweep keeps the file for good; with the take in the
        // project it is deleted, and `materialize` writes it again.
        let p = project();
        let (_d, store) = store_with(Some((TRUE_LATENCY_MS, Source::Measured)));
        run_with(&p, &Fake::new(), &store, &req()).unwrap();
        let (punch_node, punched, bytes) = {
            let handle = p.state.store_handle().unwrap();
            let st = handle.lock().unwrap();
            let head = st.head().unwrap();
            let path = st.get(head).unwrap().state.tracks[0].clips[0]
                .source_path
                .clone();
            (head, path.clone(), std::fs::read(path).unwrap())
        };
        for (a, b) in [(0.1, 0.2), (3.2, 3.3)] {
            crate::commands::run_track_tool(
                &p.state,
                "silence_region",
                json!({ "track": 0, "start_sec": a, "end_sec": b }),
            )
            .unwrap();
        }

        let handle = p.state.store_handle().unwrap();
        let st = handle.lock().unwrap();
        let report = tools::reclaim::sweep(&st, 0).expect("sweep");
        assert_eq!(report.kept_unverified, 0, "{report:?}");
        assert!(!punched.is_file(), "the punch's audio was kept: {report:?}");
        tools::rederive::materialize(&st, punch_node).expect("rebuilt");
        assert!(
            std::fs::read(&punched).expect("back") == bytes,
            "and came back byte for byte"
        );
    }
}
