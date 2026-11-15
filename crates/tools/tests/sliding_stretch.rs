//! Sliding stretch through the dispatcher: `time_stretch` with `end_factor`
//! and `pitch_shift` with `end_semitones`, over a track or a `range`. What is
//! checked is what a listener or the session would notice: the length the
//! ramp must have, the pitch it must sweep through, a constant ramp being the
//! plain call, and what moves with the audio.

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

fn stereo_session(rate: u32, seconds: f32) -> Session {
    let mut s = Session::empty();
    let path = s.dir.path().join("stereo.wav");
    harness::write_frames(&path, rate, 2, (rate as f32 * seconds) as usize, |n| {
        let t = (n / 2) as f32 / rate as f32;
        let hz = if n % 2 == 0 { 300.0 } else { 700.0 };
        (2.0 * std::f32::consts::PI * hz * t).sin() * 0.4
    });
    s.load(&path);
    s
}

fn freq_in(x: &[f32], rate: u32, a: usize, b: usize) -> f32 {
    let s = &x[a..b];
    let c = s
        .windows(2)
        .filter(|w| (w[0] <= 0.0) != (w[1] <= 0.0))
        .count();
    (c as f32 / 2.0) * rate as f32 / s.len() as f32
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

fn ideal(span: f64, f0: f64, f1: f64) -> f64 {
    audio_time::ramp_output_position(span, span, f0, f1)
}

#[test]
fn a_whole_track_ramp_is_as_long_as_the_integral_of_the_ratio() {
    for (rate, f0, f1) in [(RATE, 1.0, 0.5), (22_050, 2.0, 0.5), (44_100, 0.5, 1.0)] {
        let mut s = tone_session(rate, 2.0, 440.0);
        let (head, nodes) = (s.head(), s.node_count());
        let out = s.ok(
            "time_stretch",
            json!({"track": 0, "factor": f0, "end_factor": f1}),
        );
        s.assert_one_node_appended_after(head, nodes);
        let audio = head_audio(&s, 0);
        assert_eq!(audio.sample_rate, rate);
        let want = ideal(2.0 * rate as f64, f0, f1);
        assert!(
            (audio.samples.len() as f64 - want).abs() <= 1.0,
            "{rate} {f0}->{f1}: {} vs {want}",
            audio.samples.len()
        );
        assert_eq!(
            out["output_seconds"].as_f64().unwrap(),
            audio.samples.len() as f64 / rate as f64
        );
        assert_eq!(out["end_value"].as_f64().unwrap(), f1);
    }
}

#[test]
fn a_stereo_ramp_keeps_two_channels_and_the_length() {
    let mut s = stereo_session(RATE, 2.0);
    s.ok(
        "time_stretch",
        json!({"track": 0, "factor": 1.0, "end_factor": 0.5}),
    );
    let audio = head_audio(&s, 0);
    assert_eq!(audio.channels, 2);
    let want = ideal(2.0 * RATE as f64, 1.0, 0.5);
    assert!((audio.samples.len() as f64 / 2.0 - want).abs() <= 1.0);
}

#[test]
fn a_constant_ramp_is_byte_identical_to_the_plain_call() {
    let run = |args: Value| {
        let mut s = tone_session(RATE, 1.0, 440.0);
        s.ok("time_stretch", args);
        head_audio(&s, 0).samples
    };
    let plain = run(json!({"track": 0, "factor": 0.8}));
    assert_eq!(
        plain,
        run(json!({"track": 0, "factor": 0.8, "end_factor": 0.8}))
    );
    // The same over a span: with and without the equal end value.
    let span = json!({"start_sec": 0.2, "end_sec": 0.7});
    assert_eq!(
        run(json!({"track": 0, "factor": 0.8, "range": span})),
        run(json!({"track": 0, "factor": 0.8, "end_factor": 0.8, "range": span}))
    );

    let pitch = |args: Value| {
        let mut s = tone_session(RATE, 1.0, 440.0);
        s.ok("pitch_shift", args);
        head_audio(&s, 0).samples
    };
    assert_eq!(
        pitch(json!({"track": 0, "semitones": 4.0})),
        pitch(json!({"track": 0, "semitones": 4.0, "end_semitones": 4.0}))
    );
}

#[test]
fn a_range_ramp_replaces_only_the_range() {
    let mut s = tone_session(RATE, 4.0, 440.0);
    let before = head_audio(&s, 0).samples;
    s.ok(
        "time_stretch",
        json!({"track": 0, "factor": 1.0, "end_factor": 0.5,
               "range": {"start_sec": 1.0, "end_sec": 3.0}}),
    );
    let after = head_audio(&s, 0).samples;
    let span = ideal(2.0 * RATE as f64, 1.0, 0.5).round() as usize;
    assert_eq!(after.len(), RATE as usize * 2 + span);
    let r = RATE as usize;
    let close = |a: &[f32], b: &[f32]| a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-3);
    assert!(close(&after[..r], &before[..r]));
    assert!(close(&after[r + span..], &before[3 * r..]));
}

#[test]
fn marks_before_inside_and_after_a_range_ramp_move_as_they_should() {
    let mut s = tone_session(RATE, 4.0, 440.0);
    s.ok("label", json!({"name": "before", "time": 0.5}));
    s.ok("label", json!({"name": "inside", "time": 2.0}));
    s.ok("label", json!({"name": "after", "time": 3.5}));
    s.ok(
        "time_stretch",
        json!({"track": 0, "factor": 1.0, "end_factor": 0.5,
               "range": {"start_sec": 1.0, "end_sec": 3.0}}),
    );
    let st = s.state();
    let total = 2.0 * 2.0 * 2f64.ln();
    assert!((mark_at(&st, "before") - 0.5).abs() < 1e-6);
    // Half way in by input time, 2 ln(0.75) / -0.5 in by output time.
    let inside = 1.0 + 2.0 * 0.75f64.ln() / -0.5;
    assert!(
        (mark_at(&st, "inside") - inside).abs() < 1e-3,
        "{}",
        mark_at(&st, "inside")
    );
    assert!((mark_at(&st, "after") - (3.5 + total - 2.0)).abs() < 1e-3);
}

#[test]
fn words_and_regions_follow_a_whole_track_ramp() {
    let mut s = tone_session(RATE, 2.0, 440.0);
    s.with_track_transcript(0, &[("a", 0.5, 1.0)]);
    let mut state = s.state();
    state.annotations.push(harness::region("r", 0.5, 1.0));
    s.append_state("region", state);
    s.ok(
        "time_stretch",
        json!({"track": 0, "factor": 1.0, "end_factor": 0.5}),
    );
    let st = s.state();
    let want = |t: f64| audio_time::ramp_output_position(t, 2.0, 1.0, 0.5);
    let w = &st.transcript.as_ref().unwrap().words[0];
    assert!((w.start_s as f64 - want(0.5)).abs() < 2e-3, "{}", w.start_s);
    assert!((w.end_s as f64 - want(1.0)).abs() < 2e-3, "{}", w.end_s);
    assert!(st.annotations.iter().any(|a| matches!(
        a.kind,
        AnnotationKind::Region { start_sec, end_sec }
            if (start_sec - want(0.5)).abs() < 2e-3 && (end_sec - want(1.0)).abs() < 2e-3
    )));
}

/// Track 0 is edited (4 s), 1 is a 1 s clip at 3 s (after a 1-2 s range),
/// 2 is a 0.5 s clip at 0 (before it).
fn synced_session(sync_lock: bool, other_rate: u32) -> Session {
    let tone = |rate: u32, n: usize| -> Vec<f32> {
        (0..n)
            .map(|i| (2.0 * std::f32::consts::PI * 440.0 * i as f32 / rate as f32).sin() * 0.4)
            .collect()
    };
    let r = other_rate as usize;
    let mut s = Session::with_tracks(&[
        (RATE, tone(RATE, RATE as usize * 4)),
        (other_rate, tone(other_rate, r)),
        (other_rate, tone(other_rate, r / 2)),
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
    let st = s.state();
    let rate = audio_decoder::decode_file(&st.tracks[track].clips[0].source_path)
        .unwrap()
        .sample_rate as f64;
    st.tracks[track].clips[0].start_in_track as f64 / rate
}

#[test]
fn sync_lock_opens_a_gap_on_the_other_tracks_for_a_slow_down() {
    let mut s = synced_session(true, RATE);
    let (head, nodes) = (s.head(), s.node_count());
    s.ok(
        "time_stretch",
        json!({"track": 0, "factor": 1.0, "end_factor": 0.5,
               "range": {"start_sec": 1.0, "end_sec": 2.0}}),
    );
    let gained = ideal(1.0, 1.0, 0.5) - 1.0;
    assert_ne!(s.head(), head);
    assert_eq!(s.node_count(), nodes + 1, "one node for every track");
    assert!((clip_start_sec(&s, 1) - (3.0 + gained)).abs() < 1e-3);
    assert!(clip_start_sec(&s, 2).abs() < 1e-6);
    assert!((mark_at(&s.state(), "late") - (3.5 + gained)).abs() < 1e-3);
}

#[test]
fn sync_lock_closes_the_gap_on_the_other_tracks_for_a_speed_up() {
    let mut s = synced_session(true, RATE);
    s.ok(
        "time_stretch",
        json!({"track": 0, "factor": 2.0, "end_factor": 2.0,
               "range": {"start_sec": 1.0, "end_sec": 2.0}}),
    );
    // The second became half a second: the others lose 0.5 s at 1.5 s.
    assert!((clip_start_sec(&s, 1) - 2.5).abs() < 1e-3);
    assert!(clip_start_sec(&s, 2).abs() < 1e-6);
    assert!((mark_at(&s.state(), "late") - 3.0).abs() < 1e-3);
}

#[test]
fn without_sync_lock_the_other_tracks_stay_but_the_marks_follow() {
    let mut s = synced_session(false, RATE);
    s.ok(
        "time_stretch",
        json!({"track": 0, "factor": 0.5, "range": {"start_sec": 1.0, "end_sec": 2.0}}),
    );
    assert!((clip_start_sec(&s, 1) - 3.0).abs() < 1e-6);
    assert!((mark_at(&s.state(), "late") - 4.5).abs() < 1e-3);
}

#[test]
fn a_whole_track_ramp_does_not_touch_other_tracks_even_with_sync_lock() {
    let mut s = synced_session(true, RATE);
    s.ok(
        "time_stretch",
        json!({"track": 0, "factor": 1.0, "end_factor": 0.5}),
    );
    assert!((clip_start_sec(&s, 1) - 3.0).abs() < 1e-6);
}

#[test]
fn other_tracks_at_other_rates_move_by_time_not_frames() {
    let mut s = synced_session(true, 22_050);
    s.ok(
        "time_stretch",
        json!({"track": 0, "factor": 0.5, "range": {"start_sec": 1.0, "end_sec": 2.0}}),
    );
    assert!((clip_start_sec(&s, 1) - 4.0).abs() < 1e-3);
}

#[test]
fn a_change_too_small_to_move_a_frame_leaves_the_other_tracks_alone() {
    // 1 kHz bed: a loss of a fraction of a frame there closes nothing.
    let mut s = synced_session(true, 1_000);
    let before = s.state().tracks[1].clips.clone();
    s.ok(
        "time_stretch",
        json!({"track": 0, "factor": 1.0, "end_factor": 1.001,
               "range": {"start_sec": 1.0, "end_sec": 1.1}}),
    );
    assert_eq!(s.state().tracks[1].clips, before);
    // And a gain too small to open a frame.
    let mut s = synced_session(true, 1_000);
    let before = s.state().tracks[1].clips.clone();
    s.ok(
        "time_stretch",
        json!({"track": 0, "factor": 1.0, "end_factor": 0.999,
               "range": {"start_sec": 1.0, "end_sec": 1.1}}),
    );
    assert_eq!(s.state().tracks[1].clips, before);
}

#[test]
fn under_sync_lock_a_track_with_two_rates_refuses_the_range_ramp() {
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
        "time_stretch",
        json!({"track": 0, "factor": 1.0, "end_factor": 0.5,
               "range": {"start_sec": 1.0, "end_sec": 2.0}}),
    );
    assert!(msg.contains("different sample rates"), "{msg}");
    assert_eq!((s.head(), s.node_count()), (head, nodes));
}

#[test]
fn a_pitch_ramp_sweeps_the_frequency_and_keeps_the_length() {
    for rate in [RATE, 22_050] {
        let mut s = tone_session(rate, 2.0, 440.0);
        let out = s.ok(
            "pitch_shift",
            json!({"track": 0, "semitones": 0.0, "end_semitones": 12.0}),
        );
        let audio = head_audio(&s, 0);
        let n = rate as usize * 2;
        assert_eq!(audio.samples.len(), n);
        assert_eq!(out["output_seconds"].as_f64().unwrap(), 2.0);
        let win = rate as usize / 5;
        for centre in [0.2f32, 0.5, 0.8] {
            let c = (n as f32 * centre) as usize;
            let want = 440.0 * 2f32.powf(centre);
            let f = freq_in(&audio.samples, rate, c - win / 2, c + win / 2);
            assert!(
                (f / want - 1.0).abs() < 0.08,
                "{rate} at {centre}: {want} vs {f}"
            );
        }
    }
}

#[test]
fn a_pitch_ramp_over_a_range_leaves_the_rest_and_moves_nothing() {
    let mut s = stereo_session(RATE, 3.0);
    s.ok("label", json!({"name": "m", "time": 2.5}));
    s.ok("set_sync_lock", json!({"enabled": true}));
    let before = head_audio(&s, 0).samples;
    s.ok(
        "pitch_shift",
        json!({"track": 0, "semitones": 0.0, "end_semitones": -5.0,
               "preserve_formants": true,
               "range": {"start_sec": 1.0, "end_sec": 2.0}}),
    );
    let after = head_audio(&s, 0);
    assert_eq!(after.channels, 2);
    assert_eq!(after.samples.len(), before.len());
    let r = RATE as usize * 2;
    assert!(after.samples[..r]
        .iter()
        .zip(&before[..r])
        .all(|(a, b)| (a - b).abs() < 1e-3));
    assert!((mark_at(&s.state(), "m") - 2.5).abs() < 1e-9);
}

#[test]
fn bad_arguments_are_refused_by_name_and_change_nothing() {
    let mut s = tone_session(RATE, 1.0, 440.0);
    let (head, nodes) = (s.head(), s.node_count());
    for (tool, args, wants) in [
        (
            "time_stretch",
            json!({"track": 0, "factor": 1.0, "end_factor": 11.0}),
            vec!["end_factor", "0.1", "10"],
        ),
        (
            "time_stretch",
            json!({"track": 0, "factor": 1.0, "end_factor": 0.05}),
            vec!["end_factor", "0.1", "10"],
        ),
        (
            "time_stretch",
            json!({"track": 0, "factor": 1.0, "end_factor": -1.0}),
            vec!["end_factor"],
        ),
        (
            "time_stretch",
            json!({"track": 0, "factor": 0.0, "end_factor": 1.0}),
            vec!["invalid factor", "0.1", "10"],
        ),
        (
            "time_stretch",
            json!({"track": 0, "factor": 20.0, "end_factor": 1.0}),
            vec!["invalid factor"],
        ),
        (
            "time_stretch",
            json!({"track": 0, "factor": 1e20, "range": {"start_sec": 0.1, "end_sec": 0.5}}),
            vec!["invalid factor", "10"],
        ),
        (
            "time_stretch",
            json!({"track": 0, "factor": 1.0, "end_factor": 2.0, "range": {"start_sec": 0.5, "end_sec": 0.2}}),
            vec!["range"],
        ),
        (
            "time_stretch",
            json!({"track": 0, "factor": 1.0, "end_factor": 2.0, "range": {"start_sec": 5.0, "end_sec": 6.0}}),
            vec!["range"],
        ),
        (
            "time_stretch",
            json!({"track": 9, "factor": 1.0, "end_factor": 2.0}),
            vec!["track"],
        ),
        (
            "pitch_shift",
            json!({"track": 0, "semitones": 0.0, "end_semitones": 49.0}),
            vec!["end_semitones", "48"],
        ),
        (
            "pitch_shift",
            json!({"track": 0, "semitones": 0.0, "end_semitones": -1e20}),
            vec!["end_semitones", "48"],
        ),
        (
            "pitch_shift",
            json!({"track": 0, "semitones": 60.0, "end_semitones": 1.0}),
            vec!["invalid semitones", "48"],
        ),
        (
            "pitch_shift",
            json!({"track": 0, "semitones": 1.0, "end_semitones": 2.0, "range": {"start_sec": 0.5, "end_sec": 0.2}}),
            vec!["range"],
        ),
        (
            "pitch_shift",
            json!({"track": 0, "semitones": 1.0, "range": {"start_sec": 3.0, "end_sec": 4.0}}),
            vec!["range"],
        ),
    ] {
        let msg = s.err(tool, args.clone());
        for want in wants {
            assert!(msg.contains(want), "{args}: `{want}` not in `{msg}`");
        }
    }
    // Past f32's range the argument parser refuses it.
    s.err(
        "time_stretch",
        json!({"track": 0, "factor": 1.0, "end_factor": 1e39}),
    );
    assert_eq!((s.head(), s.node_count()), (head, nodes));
}

#[test]
fn a_result_over_twenty_minutes_is_refused_with_the_limit() {
    let mut s = tone_session(8_000, 150.0, 440.0);
    // 150 s at a tenth of the speed is 1500 s.
    let msg = s.err(
        "time_stretch",
        json!({"track": 0, "factor": 0.1, "end_factor": 0.11}),
    );
    assert!(msg.contains("1200"), "{msg}");
    let msg = s.err(
        "time_stretch",
        json!({"track": 0, "factor": 0.1, "range": {"start_sec": 0.0, "end_sec": 150.0}}),
    );
    assert!(msg.contains("1200"), "{msg}");
}

#[test]
fn an_empty_project_is_refused_and_says_to_load_first() {
    let mut s = Session::empty();
    for (tool, args) in [
        (
            "time_stretch",
            json!({"track": 0, "factor": 1.0, "end_factor": 0.5}),
        ),
        (
            "pitch_shift",
            json!({"track": 0, "semitones": 1.0, "end_semitones": 2.0}),
        ),
    ] {
        let msg = s.err(tool, args);
        assert!(msg.contains("no session loaded"), "{msg}");
    }
}
