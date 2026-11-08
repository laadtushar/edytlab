//! #765: the graphic EQ on `eq` (`graphic`: ten gains at the ISO octave
//! centres), measured as a frequency response through the tool.

use std::path::Path;

use serde_json::{json, Value};

#[path = "support/harness.rs"]
mod harness;
use harness::Session;

const AMP: f32 = 0.1;
const CENTRES: [f32; 10] = [
    31.5, 63.0, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0,
];

/// Ten gains, 0 dB except `(index, dB)` pairs.
fn gains(set: &[(usize, f64)]) -> Value {
    let mut g = [0.0f64; 10];
    for &(i, db) in set {
        g[i] = db;
    }
    json!(g)
}

/// RMS in dB of every `stride`-th sample from `offset`, over the second half
/// (past the filters' settling).
fn tail_db(pcm: &[i16], stride: usize, offset: usize) -> f64 {
    let ch: Vec<f64> = pcm
        .iter()
        .skip(offset)
        .step_by(stride)
        .map(|&x| f64::from(x) / 32768.0)
        .collect();
    let tail = &ch[ch.len() / 2..];
    10.0 * (tail.iter().map(|x| x * x).sum::<f64>() / tail.len() as f64).log10()
}

fn clip_pcm(s: &Session, track: usize) -> Vec<i16> {
    let st = s.state();
    harness::read_pcm(Path::new(&st.tracks[track].clips[0].source_path))
}

/// Gain in dB `graphic` gives a mono sine of `hz` at `rate`, and the result.
fn graphic_gain_db(rate: u32, hz: f32, graphic: Value) -> (f64, Value) {
    let mut s = Session::with_tones(rate, rate as usize, &[(hz, AMP)]);
    let before = tail_db(&clip_pcm(&s, 0), 1, 0);
    let out = s.ok("eq", json!({ "track": 0, "graphic": graphic }));
    (tail_db(&clip_pcm(&s, 0), 1, 0) - before, out)
}

fn assert_near(got: f64, want: f64, tol: f64, what: &str) {
    assert!(
        (got - want).abs() <= tol,
        "{what}: got {got:.2} dB, want {want} ± {tol}"
    );
}

#[test]
fn each_centre_gets_its_gain_at_44k1_and_22k05() {
    // At 22.05 kHz the 16 kHz band is above what the track holds (below).
    for (rate, bands) in [(44_100u32, 10usize), (22_050, 9)] {
        for (i, &hz) in CENTRES.iter().enumerate().take(bands) {
            let (db, _) = graphic_gain_db(rate, hz, gains(&[(i, 6.0)]));
            assert_near(db, 6.0, 0.25, &format!("{hz} Hz at {rate}"));
        }
    }
}

#[test]
fn a_band_leaves_frequencies_two_octaves_away_nearly_alone() {
    // One octave of bandwidth: two octaves out, a +6 dB band adds ~0.2 dB.
    for hz in [250.0, 4000.0] {
        let (db, _) = graphic_gain_db(44_100, hz, gains(&[(5, 6.0)]));
        assert_near(db, 0.2, 0.3, &format!("{hz} Hz under a 1 kHz band"));
    }
}

#[test]
fn midway_between_two_boosted_centres_their_halves_add_up() {
    // Each band gives half its gain half an octave out (the cookbook's
    // bandwidth edge for a peaking EQ), so two +6 dB neighbours make +6 dB
    // at their geometric mean.
    let mid = (1000.0f32 * 2000.0).sqrt();
    for rate in [44_100, 22_050] {
        let (db, _) = graphic_gain_db(rate, mid, gains(&[(5, 6.0), (6, 6.0)]));
        assert_near(db, 6.0, 0.35, &format!("{mid} Hz at {rate}"));
    }
}

#[test]
fn the_top_octaves_keep_an_octave_of_bandwidth() {
    // A fixed analog Q of sqrt(2) narrows a band near Nyquist (the
    // cookbook's digital relation scales the bandwidth by w0/sin(w0)), so
    // two +6 dB neighbours up top would leave a dip between them (3.0 dB
    // midway between 8 kHz and 16 kHz at 44.1 kHz). With the warped Q the
    // midpoint is within 1 dB of +6 dB: near Nyquist the bilinear
    // transform makes each peak lopsided, so it is not exactly +6 dB.
    let mid = (8000.0f32 * 16000.0).sqrt();
    let (db, _) = graphic_gain_db(44_100, mid, gains(&[(8, 6.0), (9, 6.0)]));
    assert_near(db, 6.0, 1.0, &format!("{mid} Hz at 44100"));
}

#[test]
fn cuts_land_as_well_as_boosts() {
    let (db, _) = graphic_gain_db(44_100, 250.0, gains(&[(3, -12.0)]));
    assert_near(db, -12.0, 0.3, "250 Hz cut");
    let (db, _) = graphic_gain_db(22_050, 63.0, gains(&[(1, -9.0), (7, 4.0)]));
    assert_near(db, -9.0, 0.3, "63 Hz cut at 22.05 kHz");
}

#[test]
fn stereo_channels_are_filtered_apart() {
    let mut s = Session::empty();
    let path = s.dir.path().join("st.wav");
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: 44_100,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let tone = |hz: f32, n: usize| {
        ((2.0 * std::f32::consts::PI * hz * n as f32 / 44_100.0).sin() * AMP * 32_767.0) as i16
    };
    let mut w = hound::WavWriter::create(&path, spec).unwrap();
    for n in 0..44_100 {
        w.write_sample(tone(1000.0, n)).unwrap();
        w.write_sample(tone(125.0, n)).unwrap();
    }
    w.finalize().unwrap();
    s.load(&path);
    let before = clip_pcm(&s, 0);
    s.ok(
        "eq",
        json!({ "track": 0, "graphic": gains(&[(5, 6.0), (2, -6.0)]) }),
    );
    let after = clip_pcm(&s, 0);
    assert_eq!(after.len(), before.len(), "still stereo, same length");
    let left = tail_db(&after, 2, 0) - tail_db(&before, 2, 0);
    let right = tail_db(&after, 2, 1) - tail_db(&before, 2, 1);
    assert_near(left, 6.0, 0.25, "left 1 kHz");
    assert_near(right, -6.0, 0.25, "right 125 Hz");
}

#[test]
fn a_band_the_rate_cannot_hold_is_skipped_and_reported() {
    // At 22.05 kHz a 16 kHz band would be pulled down to the filter's
    // ceiling (0.45 x rate, 9,922 Hz) and boost 9 kHz instead.
    let (db, out) = graphic_gain_db(22_050, 9000.0, gains(&[(9, 9.0), (5, 6.0)]));
    assert_near(db, 0.0, 0.2, "9 kHz with the 16 kHz band skipped");
    assert_eq!(out["skipped_hz"], json!([16000.0]), "{out}");

    // At 44.1 kHz nothing is skipped and the key is absent.
    let (_, out) = graphic_gain_db(44_100, 1000.0, gains(&[(9, 9.0)]));
    assert!(out.get("skipped_hz").is_none(), "{out}");
}

#[test]
fn zero_db_bands_are_left_out_of_the_node() {
    let (_, out) = graphic_gain_db(44_100, 1000.0, gains(&[(5, 6.0)]));
    let summary = out["summary"].as_str().unwrap();
    assert!(summary.contains("1000Hz +6.0dB Q1.41"), "{summary}");
    assert_eq!(summary.matches("Hz").count(), 1, "one band: {summary}");
}

#[test]
fn nothing_to_change_is_refused_and_adds_no_node() {
    let mut s = Session::with_tones(22_050, 22_050, &[(1000.0, AMP)]);
    let (head, count) = (s.head(), s.node_count());
    let m = s.err("eq", json!({ "track": 0, "graphic": gains(&[]) }));
    assert!(m.contains("nothing to change"), "{m}");
    // The only non-zero band is one this rate cannot hold.
    let m = s.err("eq", json!({ "track": 0, "graphic": gains(&[(9, 6.0)]) }));
    assert!(m.contains("16000"), "{m}");
    assert_eq!((s.head(), s.node_count()), (head, count));
}

#[test]
fn bands_and_graphic_are_one_or_the_other_and_graphic_has_ten_gains() {
    let mut s = Session::with_tones(44_100, 4410, &[(1000.0, AMP)]);
    let (head, count) = (s.head(), s.node_count());
    let band = json!([{ "freq_hz": 1000.0, "gain_db": 3.0 }]);

    let m = s.err(
        "eq",
        json!({ "track": 0, "bands": band, "graphic": gains(&[(5, 3.0)]) }),
    );
    assert!(m.contains("not both"), "{m}");
    let m = s.err("eq", json!({ "track": 0 }));
    assert!(m.contains("bands or graphic"), "{m}");

    for n in [9usize, 11] {
        let r = s.try_call("eq", json!({ "track": 0, "graphic": vec![1.0; n] }));
        assert!(
            matches!(r, Err(_) | Ok(tools::ToolResult::Error(_))),
            "{n} gains: {r:?}"
        );
    }
    assert_eq!((s.head(), s.node_count()), (head, count), "no node");

    // Each on its own is accepted.
    s.ok("eq", json!({ "track": 0, "bands": band }));
    s.ok("eq", json!({ "track": 0, "graphic": gains(&[(5, 3.0)]) }));
}
