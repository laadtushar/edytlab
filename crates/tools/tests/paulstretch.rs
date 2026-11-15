//! `paulstretch` through the dispatcher: what a listener would check of an
//! extreme stretch (how long it is, how loud, what pitch) and what the
//! session keeps (one node, the marks moved, a refusal that changes nothing).

use hound::{SampleFormat, WavSpec, WavWriter};
use realfft::RealFftPlanner;
use serde_json::{json, Value};
use session::{AnnotationKind, SessionState};

#[path = "support/harness.rs"]
mod harness;
use harness::{write_sine, Session};

const RATE: u32 = 48_000;

fn head_audio(s: &Session, track: usize) -> audio_decoder::DecodedAudio {
    let state = s.state();
    let clips = &state.tracks[track].clips;
    assert_eq!(clips.len(), 1, "a destructive edit leaves one clip");
    audio_decoder::decode_file(&clips[0].source_path).unwrap()
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|s| s * s).sum::<f32>() / x.len() as f32).sqrt()
}

fn peak_hz(x: &[f32], rate: u32) -> f32 {
    let n = 1 << 15;
    let mut buf: Vec<f32> = x[..n]
        .iter()
        .enumerate()
        .map(|(i, s)| s * (0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / n as f32).cos()))
        .collect();
    let fft = RealFftPlanner::<f32>::new().plan_fft_forward(n);
    let mut spec = fft.make_output_vec();
    fft.process(&mut buf, &mut spec).unwrap();
    let (bin, _) = spec
        .iter()
        .enumerate()
        .skip(1)
        .max_by(|a, b| a.1.norm().total_cmp(&b.1.norm()))
        .unwrap();
    bin as f32 * rate as f32 / n as f32
}

fn stereo_wav(s: &Session, rate: u32, seconds: f32, l_hz: f32, r_hz: f32) -> std::path::PathBuf {
    let path = s.dir.path().join("stereo.wav");
    let spec = WavSpec {
        channels: 2,
        sample_rate: rate,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut w = WavWriter::create(&path, spec).unwrap();
    for n in 0..(rate as f32 * seconds) as usize {
        let t = n as f32 / rate as f32;
        for (hz, amp) in [(l_hz, 0.4), (r_hz, 0.2)] {
            let v = (2.0 * std::f32::consts::PI * hz * t).sin() * amp;
            w.write_sample((v * 32_767.0) as i16).unwrap();
        }
    }
    w.finalize().unwrap();
    path
}

fn tone_session(rate: u32, seconds: f32, hz: f32) -> Session {
    let mut s = Session::empty();
    let path = write_sine(
        &s.dir.path().join("tone.wav"),
        rate,
        hz,
        0.4,
        (rate as f32 * seconds) as usize,
    );
    s.load(&path);
    s
}

#[test]
fn the_track_is_the_factor_longer_and_one_node_is_appended() {
    let mut s = tone_session(RATE, 2.0, 440.0);
    let (head, nodes) = (s.head(), s.node_count());
    let out = s.ok("paulstretch", json!({"track": 0, "factor": 4.0, "seed": 1}));
    s.assert_one_node_appended_after(head, nodes);
    assert!(out["summary"].as_str().unwrap().contains("paulstretch"));
    let audio = head_audio(&s, 0);
    assert_eq!(audio.sample_rate, RATE);
    assert_eq!(audio.samples.len(), RATE as usize * 8);
    assert_eq!(out["output_seconds"].as_f64().unwrap(), 8.0);
    assert_eq!(out["window_sec"].as_f64().unwrap(), 16_384.0 / RATE as f64);
    // The session is as long as its longest track.
    assert_eq!(s.state().length_samples, RATE as u64 * 8);
}

#[test]
fn a_steady_tone_keeps_its_level_and_its_pitch() {
    let mut s = tone_session(RATE, 3.0, 880.0);
    let before = rms(&head_audio(&s, 0).samples);
    s.ok("paulstretch", json!({"track": 0, "factor": 8.0}));
    let out = head_audio(&s, 0).samples;
    assert!(
        (rms(&out) / before - 1.0).abs() < 0.1,
        "rms {before} became {}",
        rms(&out)
    );
    let hz = peak_hz(&out[RATE as usize..], RATE);
    assert!((hz - 880.0).abs() < 4.0, "880 Hz came out at {hz} Hz");
}

#[test]
fn a_fixed_seed_is_repeatable_and_the_default_is_too() {
    let run = |args: Value| {
        let mut s = tone_session(RATE, 1.0, 440.0);
        s.ok("paulstretch", args);
        head_audio(&s, 0).samples
    };
    let a = run(json!({"track": 0, "factor": 3.0, "seed": 99}));
    assert_eq!(a, run(json!({"track": 0, "factor": 3.0, "seed": 99})));
    assert_ne!(a, run(json!({"track": 0, "factor": 3.0, "seed": 100})));
    // A replayed node must give the same audio, so no seed is a seed.
    assert_eq!(
        run(json!({"track": 0, "factor": 3.0})),
        run(json!({"track": 0, "factor": 3.0}))
    );
}

#[test]
fn stereo_keeps_two_channels_and_each_side_its_pitch() {
    let mut s = Session::empty();
    let wav = stereo_wav(&s, RATE, 2.0, 300.0, 900.0);
    s.load(&wav);
    s.ok(
        "paulstretch",
        json!({"track": 0, "factor": 5.0, "window_sec": 0.2}),
    );
    let audio = head_audio(&s, 0);
    assert_eq!(audio.channels, 2);
    assert_eq!(audio.samples.len(), RATE as usize * 10 * 2);
    let l: Vec<f32> = audio.samples.iter().step_by(2).copied().collect();
    let r: Vec<f32> = audio.samples.iter().skip(1).step_by(2).copied().collect();
    assert!((peak_hz(&l[RATE as usize..], RATE) - 300.0).abs() < 4.0);
    assert!((peak_hz(&r[RATE as usize..], RATE) - 900.0).abs() < 4.0);
    assert!(rms(&l) > 1.5 * rms(&r), "the louder side stays louder");
}

#[test]
fn another_sample_rate_is_stretched_in_seconds() {
    let mut s = tone_session(22_050, 1.0, 500.0);
    let out = s.ok("paulstretch", json!({"track": 0, "factor": 6.0}));
    let audio = head_audio(&s, 0);
    assert_eq!(audio.sample_rate, 22_050);
    assert_eq!(audio.samples.len(), 22_050 * 6);
    assert_eq!(out["output_seconds"].as_f64().unwrap(), 6.0);
    assert!((peak_hz(&audio.samples[22_050..], 22_050) - 500.0).abs() < 3.0);
}

#[test]
fn a_range_is_stretched_and_the_rest_is_kept() {
    let mut s = tone_session(RATE, 4.0, 440.0);
    let before = head_audio(&s, 0).samples;
    s.ok(
        "paulstretch",
        json!({"track": 0, "factor": 3.0, "range": {"start_sec": 1.0, "end_sec": 2.0}}),
    );
    let after = head_audio(&s, 0).samples;
    // 1 s before + 1 s x 3 + 2 s after.
    assert_eq!(after.len(), RATE as usize * 6);
    let rate = RATE as usize;
    let close = |a: &[f32], b: &[f32]| a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-3);
    assert!(close(&after[..rate], &before[..rate]), "the start moved");
    assert!(
        close(&after[4 * rate..], &before[2 * rate..]),
        "the end moved"
    );
    assert!(
        rms(&after[rate + 4_000..4 * rate - 4_000]) > 0.2,
        "silent middle"
    );
}

#[test]
fn a_range_comes_from_the_chat_message_prefix_too() {
    let mut s = tone_session(RATE, 4.0, 440.0);
    let r = s.call_saying(
        "paulstretch",
        json!({"track": 0, "factor": 2.0}),
        "[apply to 0:01-0:02] make it a pad",
    );
    assert!(matches!(r, tools::ToolResult::Ok(_)), "{r:?}");
    assert_eq!(head_audio(&s, 0).samples.len(), RATE as usize * 5);
}

#[test]
fn marks_move_with_the_stretch() {
    // Whole track: a mark at 1 s of a 4x stretch belongs at 4 s.
    let mut s = tone_session(RATE, 2.0, 440.0);
    s.ok("label", json!({"name": "m", "time": 1.0}));
    s.ok("paulstretch", json!({"track": 0, "factor": 4.0}));
    assert!((mark_at(&s.state(), "m") - 4.0).abs() < 1e-3);

    // A range: a mark after it moves by what the range gained, one before stays.
    let mut s = tone_session(RATE, 4.0, 440.0);
    s.ok("label", json!({"name": "before", "time": 0.5}));
    s.ok("label", json!({"name": "after", "time": 3.0}));
    s.ok(
        "paulstretch",
        json!({"track": 0, "factor": 3.0, "range": {"start_sec": 1.0, "end_sec": 2.0}}),
    );
    assert!((mark_at(&s.state(), "before") - 0.5).abs() < 1e-3);
    assert!((mark_at(&s.state(), "after") - 5.0).abs() < 1e-3);
}

fn mark_at(state: &SessionState, name: &str) -> f64 {
    let a = state
        .annotations
        .iter()
        .find(|a| a.name == name)
        .expect("mark");
    match &a.kind {
        AnnotationKind::Marker { time_sec } => *time_sec,
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_new_audio_is_written_under_the_projects_derived_folder() {
    // There is no destination argument, so `refuse_project_audio` has nothing
    // to check: the only file written is the derived WAV, never a source.
    let mut s = tone_session(RATE, 1.0, 440.0);
    let source = s.state().tracks[0].clips[0].source_path.clone();
    let source_bytes = std::fs::read(&source).unwrap();
    s.ok("paulstretch", json!({"track": 0, "factor": 2.0}));
    let new = s.state().tracks[0].clips[0].source_path.clone();
    assert_ne!(new, source);
    assert!(new.starts_with(tools::provenance::derived_dir(s.store.project_dir())));
    assert_eq!(std::fs::read(&source).unwrap(), source_bytes);
}

#[test]
fn bad_arguments_are_refused_and_change_nothing() {
    let mut s = tone_session(RATE, 1.0, 440.0);
    let (head, nodes) = (s.head(), s.node_count());
    for (args, want) in [
        (json!({"track": 0, "factor": 0.5}), "stretch"),
        (json!({"track": 0, "factor": 0.0}), "stretch"),
        (json!({"track": 0, "factor": -3.0}), "stretch"),
        (json!({"track": 0, "factor": 50.5}), "stretch"),
        (
            json!({"track": 0, "factor": 4.0, "window_sec": 0.01}),
            "window",
        ),
        (
            json!({"track": 0, "factor": 4.0, "window_sec": 5.5}),
            "window",
        ),
        (
            json!({"track": 0, "factor": 4.0, "window_sec": -1.0}),
            "window",
        ),
        (json!({"track": 9, "factor": 4.0}), "track"),
        (
            json!({"track": 0, "factor": 4.0, "range": {"start_sec": 2.0, "end_sec": 1.0}}),
            "range",
        ),
        (
            json!({"track": 0, "factor": 4.0, "range": {"start_sec": 5.0, "end_sec": 6.0}}),
            "range",
        ),
    ] {
        let msg = s.err("paulstretch", args.clone());
        assert!(msg.contains(want), "{args}: {msg}");
    }
    // A missing factor is the schema's refusal, before the tool runs.
    let msg = s.try_call("paulstretch", json!({"track": 0})).unwrap_err();
    assert!(msg.to_string().contains("factor"), "{msg}");
    assert_eq!(s.head(), head);
    assert_eq!(s.node_count(), nodes);
}

#[test]
fn an_output_longer_than_twenty_minutes_is_refused_with_the_limit() {
    let mut s = tone_session(8_000, 30.0, 440.0);
    // 30 s x 50 = 25 minutes.
    let msg = s.err("paulstretch", json!({"track": 0, "factor": 50.0}));
    assert!(msg.contains("1200"), "{msg}");
    assert!(msg.contains("1500"), "{msg}");
}

/// What a refused call must leave behind: the head and the node count.
fn refused_with(args: Value, wants: &[&str]) {
    let mut s = tone_session(RATE, 1.0, 440.0);
    let (head, nodes) = (s.head(), s.node_count());
    let msg = match s.try_call("paulstretch", args.clone()) {
        Ok(tools::ToolResult::Error(m)) => m,
        Ok(tools::ToolResult::Ok(v)) => panic!("{args}: expected a refusal, got {v}"),
        Err(e) => e.to_string(),
    };
    for want in wants {
        assert!(msg.contains(want), "{args}: `{want}` not in `{msg}`");
    }
    assert_eq!(s.head(), head, "{args}");
    assert_eq!(s.node_count(), nodes, "{args}");
}

#[test]
fn a_huge_factor_is_refused_before_any_arithmetic_with_its_bounds() {
    // 1e20 saturates `as usize`; it must be refused by name, not overflow.
    for bad in [1e20, 1e30, 50.01] {
        refused_with(
            json!({"track": 0, "factor": bad, "range": {"start_sec": 0.1, "end_sec": 0.9}}),
            &["stretch factor", "1", "50"],
        );
        refused_with(
            json!({"track": 0, "factor": bad}),
            &["stretch factor", "1", "50"],
        );
    }
    // Past f32's range the argument parser refuses it; still no node.
    refused_with(json!({"track": 0, "factor": 1e39}), &[]);
    refused_with(
        json!({"track": 0, "factor": 0.999}),
        &["stretch factor", "1", "50"],
    );
}

#[test]
fn a_window_outside_its_bounds_is_refused_with_them() {
    for bad in [0.0, 0.0199, 5.01, 1e20, -0.25] {
        refused_with(
            json!({"track": 0, "factor": 4.0, "window_sec": bad}),
            &["window_sec", "0.02", "5"],
        );
    }
}

/// Three tracks: 0 is edited, 1 is a 1 s clip moved to start at 3 s (after the
/// range), 2 is a 0.5 s clip at 0 (before it).
fn synced_session(sync_lock: bool) -> Session {
    let tone = |n: usize| -> Vec<f32> {
        (0..n)
            .map(|i| (2.0 * std::f32::consts::PI * 440.0 * i as f32 / RATE as f32).sin() * 0.4)
            .collect()
    };
    let mut s = Session::with_tracks(&[
        (RATE, tone(RATE as usize * 4)),
        (RATE, tone(RATE as usize)),
        (RATE, tone(RATE as usize / 2)),
    ]);
    s.ok(
        "move_clip",
        json!({"track": 1, "clip_index": 0, "start_sec": 3.0}),
    );
    s.ok("label", json!({"name": "late", "time": 3.5}));
    s.ok("set_sync_lock", json!({"enabled": sync_lock}));
    s
}

fn clip_start_sec(s: &Session, track: usize) -> f64 {
    s.state().tracks[track].clips[0].start_in_track as f64 / RATE as f64
}

#[test]
fn a_range_stretch_with_sync_lock_opens_the_gap_on_the_other_tracks() {
    let mut s = synced_session(true);
    let (head, nodes) = (s.head(), s.node_count());
    s.ok(
        "paulstretch",
        json!({"track": 0, "factor": 3.0, "range": {"start_sec": 1.0, "end_sec": 2.0}}),
    );
    assert_ne!(s.head(), head);
    // The range gained 1 s x (3 - 1) = 2 s at 2 s.
    assert!(
        (clip_start_sec(&s, 1) - 5.0).abs() < 1e-6,
        "after the range"
    );
    assert!(clip_start_sec(&s, 2).abs() < 1e-6, "before the range");
    assert!((mark_at(&s.state(), "late") - 5.5).abs() < 1e-3);
    // One node covers every track it moved, and the moved track's clip is
    // shifted, not split.
    assert_eq!(s.node_count(), nodes + 1);
    assert_eq!(s.state().tracks[1].clips.len(), 1);
}

#[test]
fn a_range_stretch_without_sync_lock_leaves_the_other_tracks() {
    let mut s = synced_session(false);
    s.ok(
        "paulstretch",
        json!({"track": 0, "factor": 3.0, "range": {"start_sec": 1.0, "end_sec": 2.0}}),
    );
    assert!((clip_start_sec(&s, 1) - 3.0).abs() < 1e-6);
    assert!(clip_start_sec(&s, 2).abs() < 1e-6);
    // Marks follow the recording whether or not the tracks are locked.
    assert!((mark_at(&s.state(), "late") - 5.5).abs() < 1e-3);
}

#[test]
fn a_whole_track_stretch_does_not_touch_other_tracks_even_with_sync_lock() {
    let mut s = synced_session(true);
    s.ok("paulstretch", json!({"track": 0, "factor": 2.0}));
    assert!((clip_start_sec(&s, 1) - 3.0).abs() < 1e-6);
}

/// Arguments that do not parse are the tool's refusal (wrong type), and the
/// session is left as it was.
#[test]
fn arguments_of_the_wrong_type_are_refused_and_change_nothing() {
    let mut s = tone_session(RATE, 1.0, 440.0);
    let (head, nodes) = (s.head(), s.node_count());
    // An integer to the schema, but not a `usize` or a `u64`: the tool's own
    // parser refuses it, as a result and not a dispatcher error.
    for (args, bad) in [
        (json!({"track": -1, "factor": 2.0}), "`-1`, expected usize"),
        (
            json!({"track": 0, "factor": 2.0, "seed": -5}),
            "`-5`, expected u64",
        ),
    ] {
        let msg = s.err("paulstretch", args);
        // The refusal names the value and the type it should have been, so
        // each case fails for its own argument, not for any refusal.
        assert!(msg.starts_with("invalid arguments:"), "{msg}");
        assert!(msg.contains(bad), "{msg}");
    }
    assert_eq!(s.head(), head);
    assert_eq!(s.node_count(), nodes);
}

#[test]
fn an_empty_project_is_refused_and_says_to_load_first() {
    let mut s = Session::empty();
    let msg = s.err("paulstretch", json!({"track": 0, "factor": 2.0}));
    assert!(msg.contains("no session loaded"), "{msg}");
}

#[test]
fn under_sync_lock_a_track_with_clips_at_two_rates_refuses_the_range_stretch() {
    let mut s = Session::with_tracks(&[
        (
            RATE,
            (0..RATE as usize * 4)
                .map(|i| (i as f32 * 0.01).sin() * 0.4)
                .collect(),
        ),
        (
            22_050,
            (0..22_050).map(|i| (i as f32 * 0.02).sin() * 0.4).collect(),
        ),
    ]);
    // Put the 48 kHz clip on the 22.05 kHz track as well: no single rate.
    let mut state = s.state();
    let extra = state.tracks[0].clips[0].clone();
    state.tracks[1].clips.push(session::Clip {
        start_in_track: 22_050,
        ..extra
    });
    s.append_state("mixed", state);
    s.ok("set_sync_lock", json!({"enabled": true}));
    let (head, nodes) = (s.head(), s.node_count());
    let msg = s.err(
        "paulstretch",
        json!({"track": 0, "factor": 3.0, "range": {"start_sec": 1.0, "end_sec": 2.0}}),
    );
    assert!(msg.contains("different sample rates"), "{msg}");
    assert_eq!(s.head(), head);
    assert_eq!(s.node_count(), nodes);
}

/// A factor of 1 gains no time, so there is no gap to open: the other tracks
/// are left exactly as they were, and the edited one is the same length.
#[test]
fn a_range_stretch_that_gains_nothing_leaves_a_sync_locked_track_alone() {
    let mut s = synced_session(true);
    let before = s.state().tracks[1].clips.clone();
    s.ok(
        "paulstretch",
        json!({"track": 0, "factor": 1.0, "range": {"start_sec": 1.0, "end_sec": 2.0}}),
    );
    assert_eq!(s.state().tracks[1].clips, before);
    assert_eq!(head_audio(&s, 0).samples.len(), RATE as usize * 4);
}
