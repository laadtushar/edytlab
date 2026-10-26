//! `measure`: the numbers a distributor checks, through the tool (#763).
//!
//! The measurements themselves are held to EBU Tech 3341 and 3342 in
//! `audio-analysis/src/qc.rs`. These tests go through the whole tool: a
//! project on disk, the render, the range in seconds and the report the
//! model reads. They check that
//!
//! * a track and the mix are measured as they play (gain included), over the
//!   range asked for, in session seconds, also when the track's file is not
//!   at the session's rate;
//! * the known signals of the spec come out the same through the render;
//! * the ACX check names each rule, its limit and what was measured, and
//!   passes and fails on files built to do so;
//! * the answer is bounded, and the tool leaves the project as it found it;
//! * the measure, clean, level and measure-again loop of the bundled
//!   `acx-audiobook` skill reaches a pass on a fixture that starts out
//!   failing two of the three rules.
//!
//! ACX's requirements: <https://help.acx.com/s/article/acx-audio-submission-requirements>.

use std::f64::consts::TAU;
use std::path::{Path, PathBuf};

use hound::{SampleFormat, WavSpec, WavWriter};
use serde_json::{json, Value};
use tempfile::TempDir;
use tools::{ToolContext, ToolDispatcher, ToolResult};

/// A mono or interleaved 16-bit WAV of `samples` (full scale is 1.0).
fn write_wav(path: &Path, rate: u32, channels: u16, samples: &[f32]) -> PathBuf {
    let spec = WavSpec {
        channels,
        sample_rate: rate,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut w = WavWriter::create(path, spec).expect("wav writer");
    for s in samples {
        w.write_sample((s.clamp(-1.0, 1.0) * 32_767.0).round() as i16)
            .unwrap();
    }
    w.finalize().unwrap();
    path.to_path_buf()
}

fn lin(db: f64) -> f64 {
    10f64.powf(db / 20.0)
}

/// `seconds` of a sine at `hz` with a peak of `peak_db` dBFS.
fn sine(rate: u32, seconds: f64, hz: f64, peak_db: f64) -> Vec<f32> {
    let a = lin(peak_db);
    (0..(seconds * rate as f64).round() as usize)
        .map(|i| (a * (TAU * hz * i as f64 / rate as f64).sin()) as f32)
        .collect()
}

/// White noise at an RMS of `rms_db` dBFS (a fixed LCG, so every run is the same).
fn noise(rate: u32, seconds: f64, rms_db: f64, seed: u64) -> Vec<f32> {
    let mut x = seed;
    let gain = lin(rms_db) * 3f64.sqrt();
    (0..(seconds * rate as f64).round() as usize)
        .map(|_| {
            x = x
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (((x >> 11) as f64 / (1u64 << 53) as f64 * 2.0 - 1.0) * gain) as f32
        })
        .collect()
}

fn stereo(mono: &[f32]) -> Vec<f32> {
    mono.iter().flat_map(|&s| [s, s]).collect()
}

fn concat(parts: &[Vec<f32>]) -> Vec<f32> {
    parts.iter().flatten().copied().collect()
}

struct Project {
    dir: TempDir,
    store: session::Store,
    engine: audio_engine::Engine,
    dispatcher: ToolDispatcher,
    clipboard: Option<tools::Clipboard>,
}

impl Project {
    fn new() -> Self {
        let dir = TempDir::new().expect("tempdir");
        let store = session::Store::open(dir.path()).expect("open store");
        Self {
            dir,
            store,
            engine: audio_engine::Engine::new(),
            dispatcher: ToolDispatcher::default_dispatcher(),
            clipboard: None,
        }
    }

    /// Write a WAV into the project folder and load it as the next track.
    fn load(&mut self, name: &str, rate: u32, channels: u16, samples: &[f32]) -> usize {
        let path = write_wav(&self.dir.path().join(name), rate, channels, samples);
        self.ok("load", json!({ "path": path.to_string_lossy() }));
        self.state().tracks.len() - 1
    }

    fn call(&mut self, tool: &str, args: Value) -> ToolResult {
        let mut ctx = ToolContext {
            store: &mut self.store,
            engine: &mut self.engine,
            user_message: "",
            clipboard: &mut self.clipboard,
            allowed_tools: None,
        };
        self.dispatcher.invoke(tool, args, &mut ctx).unwrap()
    }

    fn ok(&mut self, tool: &str, args: Value) -> Value {
        match self.call(tool, args) {
            ToolResult::Ok(v) => v,
            ToolResult::Error(m) => panic!("{tool}: expected Ok, got Error({m})"),
        }
    }

    fn err(&mut self, tool: &str, args: Value) -> String {
        match self.call(tool, args) {
            ToolResult::Error(m) => m,
            ToolResult::Ok(v) => panic!("{tool}: expected Error, got Ok({v})"),
        }
    }

    fn state(&self) -> session::SessionState {
        let head = self.store.head().expect("a head");
        self.store.get(head).expect("head node").state
    }
}

fn num(v: &Value, path: &[&str]) -> f64 {
    let mut at = v;
    for key in path {
        at = &at[*key];
    }
    at.as_f64()
        .unwrap_or_else(|| panic!("{path:?} is not a number in {v}"))
}

fn close(what: &str, got: f64, want: f64, tol: f64) {
    eprintln!("{what}: measured {got:.3}, reference {want} +/- {tol}");
    assert!(
        (got - want).abs() <= tol,
        "{what}: {got} vs {want} +/- {tol}"
    );
}

// ---- the signals of the spec, through the tool -----------------------------

/// Tech 3341 case 1 through the tool: a stereo 1 kHz sine at -23.0 dBFS per
/// channel, 20 s: I, M max and S max are -23.0 +/- 0.1 LUFS.
#[test]
fn the_spec_tone_reads_minus_23_lufs() {
    let mut p = Project::new();
    p.load(
        "tone.wav",
        48_000,
        2,
        &stereo(&sine(48_000, 20.0, 1000.0, -23.0)),
    );
    let r = p.ok("measure", json!({ "track": 0 }));
    close("I", num(&r, &["loudness", "integrated_lufs"]), -23.0, 0.1);
    close(
        "M max",
        num(&r, &["loudness", "momentary_max_lufs"]),
        -23.0,
        0.1,
    );
    close(
        "S max",
        num(&r, &["loudness", "short_term_max_lufs"]),
        -23.0,
        0.1,
    );
    close("RMS", num(&r, &["rms_dbfs"]), -26.02, 0.05);
    close(
        "sample peak",
        num(&r, &["peak", "sample_dbfs"]),
        -23.0,
        0.05,
    );
    assert_eq!(r["sample_rate"], 48_000);
    assert_eq!(r["channels"], 2);
}

/// Tech 3342 case 1 through the tool: 20 s at -20 dBFS then 20 s at -30:
/// LRA = 10 +/- 1 LU.
#[test]
fn the_spec_two_level_signal_reads_10_lu_of_range() {
    let mut p = Project::new();
    let s = concat(&[
        stereo(&sine(48_000, 20.0, 1000.0, -20.0)),
        stereo(&sine(48_000, 20.0, 1000.0, -30.0)),
    ]);
    p.load("range.wav", 48_000, 2, &s);
    let r = p.ok("measure", json!({ "track": 0 }));
    close("LRA", num(&r, &["loudness", "range_lu"]), 10.0, 1.0);
}

/// Tech 3341 case 16 through the tool: fs/4 at 45 degrees, 0.5 of full
/// scale. Every sample is +/-0.354 (-9.03 dBFS) and the waveform peaks at
/// -6.0 dBTP (+0.2/-0.4).
#[test]
fn the_inter_sample_peak_signal_reads_its_true_peak() {
    let mut p = Project::new();
    let n = 48_000 * 3;
    let s: Vec<f32> = (0..n)
        .map(|i| (0.5 * (TAU * i as f64 / 4.0 + 45f64.to_radians()).sin()) as f32)
        .collect();
    p.load("fs4.wav", 48_000, 1, &s);
    let r = p.ok("measure", json!({ "track": 0 }));
    let sample = num(&r, &["peak", "sample_dbfs"]);
    let truth = num(&r, &["peak", "true_dbtp"]);
    close("sample peak", sample, -9.03, 0.05);
    eprintln!("true peak: measured {truth:.3}, reference -6.0 +0.2/-0.4");
    assert!((-6.4..=-5.8).contains(&truth), "true peak {truth}");
}

/// Flat tops at known times are counted and placed in session seconds,
/// whatever range is asked for.
#[test]
fn clipping_is_counted_and_placed_in_session_seconds() {
    let mut p = Project::new();
    let mut s = sine(44_100, 4.0, 220.0, -6.0);
    for t in [1.0, 2.5] {
        let at = (t * 44_100.0) as usize;
        for v in &mut s[at..at + 12] {
            *v = 1.0;
        }
    }
    p.load("clipped.wav", 44_100, 1, &s);

    let all = p.ok("measure", json!({ "track": 0 }));
    assert_eq!(all["clipping"]["events"], 2, "{all}");
    let times = all["clipping"]["first_times_sec"].as_array().unwrap();
    close("first clip", times[0].as_f64().unwrap(), 1.0, 0.002);
    close("second clip", times[1].as_f64().unwrap(), 2.5, 0.002);

    // From 2 s on, one clip, and it is still at 2.5 s of the session.
    let late = p.ok("measure", json!({ "track": 0, "start_sec": 2.0 }));
    assert_eq!(late["clipping"]["events"], 1, "{late}");
    close(
        "clip in range",
        late["clipping"]["first_times_sec"][0].as_f64().unwrap(),
        2.5,
        0.002,
    );
}

// ---- track, mix, gain, range -----------------------------------------------

/// A track is measured as it plays: its own gain is in the number.
#[test]
fn a_track_is_measured_with_its_gain() {
    let mut p = Project::new();
    p.load("a.wav", 48_000, 1, &sine(48_000, 3.0, 1000.0, -20.0));
    let before = p.ok("measure", json!({ "track": 0 }));
    close("peak", num(&before, &["peak", "sample_dbfs"]), -20.0, 0.05);
    p.ok("set_track_gain", json!({ "track": 0, "db": 6.0 }));
    let after = p.ok("measure", json!({ "track": 0 }));
    close(
        "peak with +6 dB",
        num(&after, &["peak", "sample_dbfs"]),
        -14.0,
        0.05,
    );
    close(
        "RMS with +6 dB",
        num(&after, &["rms_dbfs"]) - num(&before, &["rms_dbfs"]),
        6.0,
        0.05,
    );
}

/// With no `track`, the mix; with one, that track alone.
#[test]
fn the_mix_is_every_track_and_a_track_is_its_own() {
    let mut p = Project::new();
    p.load("a.wav", 48_000, 1, &sine(48_000, 4.0, 440.0, -20.0));
    p.load("b.wav", 48_000, 1, &sine(48_000, 4.0, 1000.0, -30.0));
    let mix = p.ok("measure", json!({}));
    let t0 = p.ok("measure", json!({ "track": 0 }));
    let t1 = p.ok("measure", json!({ "track": 1 }));
    assert_eq!(mix["source"], "mix");
    assert_eq!(t1["source"], "track");
    assert_eq!(t1["track"], 1);
    close("track 0 RMS", num(&t0, &["rms_dbfs"]), -23.01, 0.05);
    close("track 1 RMS", num(&t1, &["rms_dbfs"]), -33.01, 0.05);
    // Two unrelated sines add in power: 10 log10(10^-2.301 + 10^-3.301).
    close("mix RMS", num(&mix, &["rms_dbfs"]), -22.60, 0.1);
    // The peaks of the two sines (0.1 and 0.0316) add to at most 0.1316.
    let mix_peak = num(&mix, &["peak", "sample_dbfs"]);
    assert!((-20.0..=-17.6).contains(&mix_peak), "mix peak {mix_peak}");
}

/// A range is in seconds of the session. Loud for 2 s, quiet for 2 s.
#[test]
fn a_range_measures_only_those_seconds() {
    let mut p = Project::new();
    p.load(
        "steps.wav",
        48_000,
        1,
        &concat(&[
            sine(48_000, 2.0, 1000.0, -10.0),
            sine(48_000, 2.0, 1000.0, -40.0),
        ]),
    );
    let first = p.ok(
        "measure",
        json!({ "track": 0, "start_sec": 0.0, "end_sec": 2.0 }),
    );
    let second = p.ok(
        "measure",
        json!({ "track": 0, "start_sec": 2.0, "end_sec": 4.0 }),
    );
    close(
        "first 2 s",
        num(&first, &["peak", "sample_dbfs"]),
        -10.0,
        0.1,
    );
    close(
        "last 2 s",
        num(&second, &["peak", "sample_dbfs"]),
        -40.0,
        0.1,
    );
    close("duration", num(&second, &["duration_sec"]), 2.0, 0.001);

    // An end past the audio is the end of it, and the report says so.
    let past = p.ok(
        "measure",
        json!({ "track": 0, "start_sec": 2.0, "end_sec": 100.0 }),
    );
    close("clamped end", num(&past, &["end_sec"]), 4.0, 0.001);
    close(
        "clamped peak",
        num(&past, &["peak", "sample_dbfs"]),
        -40.0,
        0.1,
    );
}

/// Seconds are seconds of the session on a track whose file is at another
/// rate (#673): a 16 kHz voice under a 48 kHz bed, loud for its first 2 s
/// and quiet for the next 2. Counting seconds in the voice's own frames at
/// the session's rate (or the other way round) would land the range three
/// times too early or late.
#[test]
fn a_range_on_a_track_at_another_rate_is_in_session_seconds() {
    let mut p = Project::new();
    p.load("bed.wav", 48_000, 1, &sine(48_000, 6.0, 220.0, -30.0));
    let voice = concat(&[
        sine(16_000, 2.0, 1000.0, -10.0),
        sine(16_000, 2.0, 1000.0, -40.0),
    ]);
    let t = p.load("voice.wav", 16_000, 1, &voice);
    assert_eq!(t, 1);
    assert_eq!(
        p.state().sample_rate,
        48_000,
        "the first load sets the rate"
    );

    let loud = p.ok(
        "measure",
        json!({ "track": 1, "start_sec": 0.5, "end_sec": 1.5 }),
    );
    let quiet = p.ok(
        "measure",
        json!({ "track": 1, "start_sec": 2.5, "end_sec": 3.5 }),
    );
    close(
        "voice, 0.5 s to 1.5 s",
        num(&loud, &["peak", "sample_dbfs"]),
        -10.0,
        0.3,
    );
    close(
        "voice, 2.5 s to 3.5 s",
        num(&quiet, &["peak", "sample_dbfs"]),
        -40.0,
        0.3,
    );
    close("duration", num(&quiet, &["duration_sec"]), 1.0, 0.001);
    assert_eq!(
        quiet["sample_rate"], 48_000,
        "what is measured is the session's mix rate"
    );

    // The mix of the same range has the bed in it.
    let mix = p.ok("measure", json!({ "start_sec": 2.5, "end_sec": 3.5 }));
    assert!(num(&mix, &["peak", "sample_dbfs"]) > -35.0, "{mix}");
}

// ---- ACX -------------------------------------------------------------------

/// A mono 44.1 kHz "narration": 1 s of room tone at `room_db` RMS, 8 s of two
/// equal sines at `burst_db` RMS, 1 s of room tone, and an optional click of
/// `click_db` dBFS in the middle.
fn narration(burst_db: f64, room_db: f64, click_db: Option<f64>) -> Vec<f32> {
    let rate = 44_100;
    // Two sines of amplitude a each have an RMS of a.
    let a = lin(burst_db);
    let burst: Vec<f32> = (0..rate * 8)
        .map(|i| {
            let t = i as f64 / rate as f64;
            (a * ((TAU * 220.0 * t).sin() + (TAU * 330.0 * t + 1.0).sin())) as f32
        })
        .collect();
    let mut s = concat(&[
        noise(rate as u32, 1.0, room_db, 1),
        burst,
        noise(rate as u32, 1.0, room_db, 2),
    ]);
    if let Some(db) = click_db {
        let at = rate * 5;
        s[at] = lin(db) as f32;
    }
    s
}

fn rule<'a>(r: &'a Value, id: &str) -> &'a Value {
    r["acx"]["rules"]
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["rule"] == id)
        .unwrap_or_else(|| panic!("no rule {id} in {r}"))
}

fn passes(r: &Value) -> (bool, bool, bool) {
    (
        rule(r, "rms")["pass"].as_bool().unwrap(),
        rule(r, "peak")["pass"].as_bool().unwrap(),
        rule(r, "noise_floor")["pass"].as_bool().unwrap(),
    )
}

#[test]
fn acx_passes_a_file_that_meets_all_three_rules() {
    let mut p = Project::new();
    // 8 s of 10 at -19 dB RMS is -19.97 dB RMS over the file.
    p.load("ok.wav", 44_100, 1, &narration(-19.0, -66.0, None));
    let r = p.ok("measure", json!({ "track": 0, "check": "acx" }));
    assert_eq!(r["acx"]["pass"], true, "{r}");
    assert_eq!(passes(&r), (true, true, true));
    close(
        "RMS",
        rule(&r, "rms")["measured"].as_f64().unwrap(),
        -19.97,
        0.2,
    );
    close(
        "noise floor",
        rule(&r, "noise_floor")["measured"].as_f64().unwrap(),
        -66.0,
        0.5,
    );
    assert!(rule(&r, "peak")["measured"].as_f64().unwrap() <= -3.0);
    for id in ["rms", "peak", "noise_floor"] {
        assert!(rule(&r, id)["requirement"].is_string(), "{id}");
    }
}

#[test]
fn acx_fails_a_file_that_is_too_quiet_and_says_by_how_much() {
    let mut p = Project::new();
    p.load("quiet.wav", 44_100, 1, &narration(-30.0, -66.0, None));
    let r = p.ok("measure", json!({ "track": 0, "check": "acx" }));
    assert_eq!(r["acx"]["pass"], false);
    assert_eq!(passes(&r), (false, true, true), "{r}");
    // Centre of -23..-18 is -20.5; the file reads about -30.97.
    close(
        "gain to the centre",
        rule(&r, "rms")["gain_to_centre_db"].as_f64().unwrap(),
        -20.5 - (-30.97),
        0.3,
    );
}

#[test]
fn acx_fails_a_peak_over_minus_3() {
    let mut p = Project::new();
    p.load("hot.wav", 44_100, 1, &narration(-19.0, -66.0, Some(-1.5)));
    let r = p.ok("measure", json!({ "track": 0, "check": "acx" }));
    assert_eq!(passes(&r), (true, false, true), "{r}");
    close(
        "peak",
        rule(&r, "peak")["measured"].as_f64().unwrap(),
        -1.5,
        0.05,
    );
}

#[test]
fn acx_fails_a_noise_floor_over_minus_60() {
    let mut p = Project::new();
    p.load("noisy.wav", 44_100, 1, &narration(-19.0, -45.0, None));
    let r = p.ok("measure", json!({ "track": 0, "check": "acx" }));
    assert_eq!(passes(&r), (true, true, false), "{r}");
    close(
        "noise floor",
        rule(&r, "noise_floor")["measured"].as_f64().unwrap(),
        -45.0,
        0.5,
    );
}

/// Each rule states its limit in words, in the order RMS, peak, noise floor.
#[test]
fn each_acx_rule_states_its_limit() {
    let mut p = Project::new();
    p.load("edge.wav", 44_100, 1, &narration(-19.0, -66.0, None));
    let r = p.ok("measure", json!({ "track": 0, "check": "acx" }));
    let limits: Vec<&str> = r["acx"]["rules"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x["requirement"].as_str().unwrap())
        .collect();
    assert_eq!(
        limits,
        [
            "-23 to -18 dBFS RMS",
            "at or below -3 dBFS peak",
            "at or below -60 dBFS RMS noise floor"
        ]
    );
}

/// The ACX block says where its limits come from and what it did not check.
#[test]
fn the_acx_block_cites_the_requirements_and_the_rules_it_leaves_out() {
    let mut p = Project::new();
    p.load("ok.wav", 44_100, 1, &narration(-19.0, -66.0, None));
    let r = p.ok("measure", json!({ "track": 0, "check": "acx" }));
    assert_eq!(
        r["acx"]["source"],
        "https://help.acx.com/s/article/acx-audio-submission-requirements"
    );
    let not_checked = r["acx"]["not_checked"].as_str().unwrap();
    assert!(
        not_checked.contains("MP3") && not_checked.contains("room tone"),
        "{not_checked}"
    );
    // Without `check` there is no verdict at all.
    let plain = p.ok("measure", json!({ "track": 0 }));
    assert!(plain.get("acx").is_none(), "{plain}");
}

// ---- the bundled skill, end to end ------------------------------------------

/// What the `acx-audiobook` skill tells the model to do, on a fixture that
/// fails: measure, reduce the noise, bring the level to the middle of the
/// band, hold the peaks, measure again.
///
/// The fixture is mono at 44.1 kHz: 2 s of room tone at -50 dBFS RMS, then
/// 8 s of "speech" at -27 dB RMS with a spike every second 10 dB above the
/// speech peaks, and 1 s of room tone. It starts out failing the noise floor
/// and the level, and after +8 dB the spikes would pass -3 dBFS, so the
/// limiter has a job.
#[test]
fn the_acx_workflow_reaches_a_pass_on_a_failing_fixture() {
    let mut p = Project::new();
    let rate = 44_100u32;
    let a = lin(-27.0);
    let speech: Vec<f32> = (0..rate as usize * 8)
        .map(|i| {
            let t = i as f64 / rate as f64;
            let spike = if i % rate as usize == 100 { 0.5 } else { 0.0 };
            (a * ((TAU * 220.0 * t).sin() + (TAU * 330.0 * t + 1.0).sin()) + spike) as f32
        })
        .collect();
    let s = concat(&[
        noise(rate, 2.0, -50.0, 11),
        speech,
        noise(rate, 1.0, -50.0, 12),
    ]);
    p.load("chapter.wav", rate, 1, &s);

    // 1. Measure.
    let before = p.ok("measure", json!({ "track": 0, "check": "acx" }));
    assert_eq!(before["acx"]["pass"], false, "{before}");
    let (rms_ok, _, noise_ok) = passes(&before);
    assert!(
        !rms_ok && !noise_ok,
        "the fixture should fail both: {before}"
    );

    // 2. Clean: learn the noise from the 2 s of room tone and measure after
    // each pass. Levelling raises the floor by the gain it adds, so the floor
    // has to end up that far under -60 before the gain, not at it.
    let mut cleaned = before.clone();
    let mut passes_made = 0;
    loop {
        let gain = rule(&cleaned, "rms")["gain_to_centre_db"]
            .as_f64()
            .unwrap_or(0.0)
            .max(0.0);
        let floor = rule(&cleaned, "noise_floor")["measured"].as_f64().unwrap();
        eprintln!("noise floor {floor} after {passes_made} pass(es), {gain} dB of gain to come");
        if floor + gain <= -60.5 || passes_made == 4 {
            break;
        }
        p.ok(
            "noise_reduction",
            json!({ "track": 0, "noise_duration_sec": 1.5, "strength": 1.0 }),
        );
        passes_made += 1;
        cleaned = p.ok("measure", json!({ "track": 0, "check": "acx" }));
    }
    assert!(passes_made >= 1, "the fixture needed cleaning");

    // 3. Level: the gain the report asked for. Then hold the peaks with the
    // limiter effect, which runs after the track's gain: the `limiter` tool
    // bakes into the file and would not see it.
    let gain = rule(&cleaned, "rms")["gain_to_centre_db"].as_f64().unwrap();
    p.ok("gain", json!({ "track": 0, "db": gain }));
    p.ok(
        "add_effect",
        json!({ "track": 0, "kind": "limiter", "params": { "ceiling_db": -3.5 } }),
    );

    // 4. Measure again.
    let after = p.ok("measure", json!({ "track": 0, "check": "acx" }));
    eprintln!(
        "ACX before: {:?}; after: {:?}",
        before["acx"]["rules"], after["acx"]["rules"]
    );
    assert_eq!(passes(&after), (true, true, true), "{after}");
    assert_eq!(after["acx"]["pass"], true);
}

// ---- bounds, errors, read-only ---------------------------------------------

/// A file with sixty clips lists fifty and counts sixty, in a result the size
/// of the others.
#[test]
fn the_answer_is_bounded_however_much_is_clipped() {
    let mut p = Project::new();
    let mut s = sine(44_100, 20.0, 220.0, -6.0);
    for k in 0..60 {
        let at = (k as f64 * 0.3 + 0.1) * 44_100.0;
        for v in &mut s[at as usize..at as usize + 6] {
            *v = 1.0;
        }
    }
    p.load("clipped.wav", 44_100, 1, &s);
    let r = p.ok(
        "measure",
        json!({ "track": 0, "check": "acx", "max_clip_times": 50 }),
    );
    assert_eq!(r["clipping"]["events"], 60);
    assert_eq!(
        r["clipping"]["first_times_sec"].as_array().unwrap().len(),
        50
    );
    let bytes = serde_json::to_string(&r).unwrap().len();
    eprintln!("result with 50 clip times and the ACX block: {bytes} bytes");
    assert!(bytes <= 2_600, "{bytes} bytes");

    // The default lists ten.
    let r = p.ok("measure", json!({ "track": 0 }));
    assert_eq!(
        r["clipping"]["first_times_sec"].as_array().unwrap().len(),
        10
    );
}

#[test]
fn bad_arguments_are_refused_with_a_reason() {
    let mut p = Project::new();
    assert!(
        p.err("measure", json!({})).contains("load"),
        "no session yet"
    );
    p.load("a.wav", 48_000, 1, &sine(48_000, 2.0, 440.0, -20.0));
    assert!(p.err("measure", json!({ "track": 5 })).contains("track"));
    assert!(p
        .err("measure", json!({ "start_sec": 2.0, "end_sec": 1.0 }))
        .contains("start_sec"));
    assert!(p
        .err("measure", json!({ "start_sec": 10.0 }))
        .contains("outside"));
}

/// The schema refuses what the tool cannot do, before the tool runs.
#[test]
fn the_schema_refuses_an_unknown_check_a_negative_time_and_too_many_clip_times() {
    let mut p = Project::new();
    p.load("a.wav", 48_000, 1, &sine(48_000, 2.0, 440.0, -20.0));
    for bad in [
        json!({ "check": "audible" }),
        json!({ "max_clip_times": 500 }),
        json!({ "start_sec": -1.0 }),
        json!({ "track": 0, "loudness": true }),
    ] {
        let mut ctx = ToolContext {
            store: &mut p.store,
            engine: &mut p.engine,
            user_message: "",
            clipboard: &mut p.clipboard,
            allowed_tools: None,
        };
        assert!(
            p.dispatcher
                .invoke("measure", bad.clone(), &mut ctx)
                .is_err(),
            "{bad} should be refused by the schema"
        );
    }
}

/// It writes nothing into the project and moves no head.
#[test]
fn measuring_changes_nothing() {
    let mut p = Project::new();
    p.load(
        "a.wav",
        48_000,
        2,
        &stereo(&sine(48_000, 3.0, 440.0, -20.0)),
    );
    p.load("b.wav", 22_050, 1, &sine(22_050, 3.0, 440.0, -20.0));
    let list = |dir: &Path| -> Vec<(String, u64)> {
        let mut out = Vec::new();
        fn walk(root: &Path, dir: &Path, out: &mut Vec<(String, u64)>) {
            for e in std::fs::read_dir(dir).unwrap() {
                let e = e.unwrap();
                if e.path().is_dir() {
                    walk(root, &e.path(), out);
                } else {
                    out.push((
                        e.path()
                            .strip_prefix(root)
                            .unwrap()
                            .to_string_lossy()
                            .into(),
                        e.metadata().unwrap().len(),
                    ));
                }
            }
        }
        walk(dir, dir, &mut out);
        out.sort();
        out
    };
    let head = p.store.head();
    let files = list(p.dir.path());
    p.ok("measure", json!({}));
    p.ok(
        "measure",
        json!({ "track": 1, "start_sec": 0.5, "end_sec": 2.0, "check": "acx" }),
    );
    assert_eq!(p.store.head(), head);
    assert_eq!(list(p.dir.path()), files, "the project folder is as it was");
    assert!(!p.dispatcher.get("measure").unwrap().mutates());
}
