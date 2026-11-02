//! #765: shelving bands on `eq` and taps on `echo`, driven through the tools.

use serde_json::json;

#[path = "support/harness.rs"]
mod harness;
use harness::Session;

const SINE_AMP: f32 = 0.1;

fn sine(rate: u32, hz: f32, frames: usize) -> Vec<f32> {
    (0..frames)
        .map(|n| (2.0 * std::f32::consts::PI * hz * n as f32 / rate as f32).sin() * SINE_AMP)
        .collect()
}

fn rms_db(v: &[i16]) -> f64 {
    let tail = &v[v.len() / 2..];
    let s: f64 = tail.iter().map(|&x| (x as f64 / 32768.0).powi(2)).sum();
    10.0 * (s / tail.len() as f64).log10()
}

/// Gain in dB the `eq` tool gives a sine of `hz` with `band`.
fn eq_gain_db(hz: f32, band: serde_json::Value) -> f64 {
    let mut s = Session::with_tracks(&[(48_000, sine(48_000, hz, 48_000))]);
    let before = rms_db(&s.render_pcm(None));
    s.ok("eq", json!({ "track": 0, "bands": [band] }));
    rms_db(&s.render_pcm(None)) - before
}

#[test]
fn low_shelf_boosts_below_the_corner_only() {
    let band = json!({ "type": "low_shelf", "freq_hz": 1000.0, "gain_db": 12.0 });
    assert!((eq_gain_db(100.0, band.clone()) - 12.0).abs() < 0.6);
    assert!(eq_gain_db(10_000.0, band).abs() < 0.3);
}

#[test]
fn high_shelf_cuts_above_the_corner_only() {
    let band = json!({ "type": "high_shelf", "freq_hz": 4000.0, "gain_db": -9.0 });
    assert!((eq_gain_db(12_000.0, band.clone()) + 9.0).abs() < 0.6);
    assert!(eq_gain_db(200.0, band).abs() < 0.3);
}

#[test]
fn a_band_without_type_is_still_a_peak() {
    let g = eq_gain_db(
        1000.0,
        json!({ "freq_hz": 1000.0, "gain_db": 6.0, "q": 1.0 }),
    );
    assert!((g - 6.0).abs() < 0.3, "peak at its centre: {g}");
}

#[test]
fn an_unknown_band_type_is_refused() {
    let mut s = Session::with_tracks(&[(48_000, sine(48_000, 100.0, 4800))]);
    // The schema's enum refuses it before the tool runs (or the tool does).
    let r = s.try_call(
        "eq",
        json!({ "track": 0, "bands": [{ "type": "bell", "freq_hz": 1000.0, "gain_db": 6.0 }] }),
    );
    assert!(matches!(r, Err(_) | Ok(tools::ToolResult::Error(_))));
}

fn impulse(frames: usize) -> Vec<f32> {
    let mut v = vec![0.0; frames];
    v[0] = 0.5;
    v
}

fn near(a: i16, want: f32) -> bool {
    (a as f32 - want * 32768.0).abs() <= 3.0
}

#[test]
fn taps_land_at_their_times_and_gains_at_each_tracks_own_rate() {
    // 10 ms and 25 ms: 80 and 200 frames at 8 kHz, 160 and 400 at 16 kHz.
    let mut s = Session::with_tracks(&[(8_000, impulse(1000)), (16_000, impulse(2000))]);
    let taps = json!([[10.0, 0.5], [25.0, -0.25]]);
    for track in 0..2 {
        s.ok("echo", json!({ "track": track, "taps": taps }));
    }
    for (track, rate) in [(0usize, 8_000usize), (1, 16_000)] {
        let st = s.state();
        let clip = &st.tracks[track].clips[0];
        let samples = harness::read_pcm(std::path::Path::new(&clip.source_path));
        let f = |ms: usize| ms * rate / 1000;
        assert!(near(samples[0], 0.5), "dry at 0");
        assert!(
            near(samples[f(10)], 0.25),
            "tap 1 at {} (rate {rate})",
            f(10)
        );
        assert!(
            near(samples[f(25)], -0.125),
            "tap 2 at {} (rate {rate})",
            f(25)
        );
        let nonzero: Vec<usize> = (0..samples.len())
            .filter(|&i| samples[i].abs() > 3)
            .collect();
        assert_eq!(
            nonzero,
            vec![0, f(10), f(25)],
            "no other energy (rate {rate})"
        );
    }
}

#[test]
fn feedback_repeats_the_tap_pattern() {
    let mut s = Session::with_tracks(&[(8_000, impulse(1000))]);
    s.ok(
        "echo",
        json!({ "track": 0, "taps": [[10.0, 0.5]], "feedback": 0.5 }),
    );
    let st = s.state();
    let samples = harness::read_pcm(std::path::Path::new(&st.tracks[0].clips[0].source_path));
    // y[n] = x[n] + g x[n-T] + fb y[n-T]: the dry hit recirculates too.
    assert!(near(samples[80], 0.5), "g*x + fb*x = 0.25 + 0.25");
    assert!(near(samples[160], 0.25), "second repeat is fb * first");
}

#[test]
fn single_echo_and_taps_are_both_valid_but_one_is_required() {
    let mut s = Session::with_tracks(&[(8_000, impulse(1000))]);
    s.ok("echo", json!({ "track": 0, "delay_ms": 10.0 }));
    s.err("echo", json!({ "track": 0 }));
    s.err("echo", json!({ "track": 0, "taps": [] }));
    s.err("echo", json!({ "track": 0, "taps": [[-5.0, 0.5]] }));
}

#[test]
fn stereo_taps_keep_channels_apart() {
    let mut s = Session::empty();
    let path = s.dir.path().join("st.wav");
    // Left carries the impulse, right is silent.
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: 8_000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut w = hound::WavWriter::create(&path, spec).unwrap();
    for n in 0..1000 {
        w.write_sample(if n == 0 { 16384i16 } else { 0 }).unwrap();
        w.write_sample(0i16).unwrap();
    }
    w.finalize().unwrap();
    s.load(&path);
    s.ok("echo", json!({ "track": 0, "taps": [[10.0, 0.5]] }));
    let st = s.state();
    let samples = harness::read_pcm(std::path::Path::new(&st.tracks[0].clips[0].source_path));
    assert!(near(samples[160], 0.25), "left tap");
    assert_eq!(samples[161], 0, "right stays silent");
}

#[test]
fn echo_arguments_outside_the_limits_are_refused_and_add_no_node() {
    let mut s = Session::with_tracks(&[(8_000, impulse(1000))]);
    let (head, count) = (s.head(), s.node_count());
    let too_many: Vec<_> = (1..=17).map(|i| json!([i as f64 * 10.0, 0.1])).collect();
    let cases = [
        // feedback past the ceiling, below zero, or not a number
        (
            json!({ "track": 0, "taps": [[10.0, 0.5]], "feedback": 0.95 }),
            "feedback",
        ),
        (
            json!({ "track": 0, "taps": [[10.0, 0.5]], "feedback": -0.1 }),
            "feedback",
        ),
        // decay belongs to the single echo
        (
            json!({ "track": 0, "taps": [[10.0, 0.5]], "decay": 0.3 }),
            "decay",
        ),
        // a tap or a single echo an hour long would allocate for it
        (
            json!({ "track": 0, "taps": [[3_600_000.0, 0.5]] }),
            "at most",
        ),
        (json!({ "track": 0, "delay_ms": 3_600_000.0 }), "at most"),
        (json!({ "track": 0, "delay_ms": 10_000.5 }), "at most"),
        (json!({ "track": 0, "taps": too_many }), "at most 16 taps"),
    ];
    for (args, want) in cases {
        let m = s.err("echo", args.clone());
        assert!(m.contains(want), "{args}: {m}");
    }
    assert_eq!(s.head(), head, "a refusal moves no head");
    assert_eq!(s.node_count(), count, "a refusal adds no node");

    // The limits themselves are accepted.
    s.ok(
        "echo",
        json!({ "track": 0, "taps": [[10_000.0, 0.5]], "feedback": 0.65 }),
    );
    let mut s = Session::with_tracks(&[(8_000, impulse(1000))]);
    let sixteen: Vec<_> = (1..=16).map(|i| json!([i as f64 * 10.0, 0.1])).collect();
    s.ok("echo", json!({ "track": 0, "taps": sixteen }));
    s.ok(
        "echo",
        json!({ "track": 0, "delay_ms": 10_000.0, "decay": 0.5 }),
    );
}

#[test]
fn only_a_shelf_names_its_kind_in_the_label() {
    let label = |band: serde_json::Value| {
        let mut s = Session::with_tracks(&[(48_000, sine(48_000, 100.0, 4800))]);
        s.ok("eq", json!({ "track": 0, "bands": [band] }));
        s.store.get(s.head()).unwrap().label.unwrap()
    };
    assert_eq!(
        label(json!({ "freq_hz": 1000.0, "gain_db": 3.0 })),
        "eq [1000Hz +3.0dB Q1.00] on track 0"
    );
    assert_eq!(
        label(json!({ "type": "peak", "freq_hz": 1000.0, "gain_db": 3.0, "q": 2.0 })),
        "eq [1000Hz +3.0dB Q2.00] on track 0"
    );
    assert_eq!(
        label(json!({ "type": "low_shelf", "freq_hz": 200.0, "gain_db": 3.0 })),
        "eq [200Hz +3.0dB Q0.71 LowShelf] on track 0"
    );
}
