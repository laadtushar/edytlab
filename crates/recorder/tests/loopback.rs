//! The clock model against real audio, end to end (#203).
//!
//! `#[ignore]`d: it needs a device pair whose output reaches its input.
//! `scripts/audio-loopback.sh` builds one without hardware (a private
//! PulseAudio daemon with a null sink, the sink's monitor as the
//! microphone) and runs these. On a machine with a sound card and its
//! speaker close to its microphone they run as they are:
//!
//! ```text
//! EDYTLAB_AUDIO_LOOPBACK=1 cargo test -p recorder --test loopback -- --ignored --nocapture
//! ```
//!
//! They fail, not skip, without `EDYTLAB_AUDIO_LOOPBACK=1`: an ignored test
//! that passes when nothing was played proves nothing.
//!
//! What they pin is the stop rule of the plan: once each run's clocks are
//! mapped, the delay is one constant. Calibrate three times: the
//! measurements agree within 1 ms. Then run again, and the stored constant
//! must predict where every sweep lands within 1 ms, while predicting with
//! no latency at all misses by more than 5 ms (a splice at the nominal
//! point would be late by that much).
//!
//! The punch-in's tests are in this file too, so the same script runs them
//! (#203, PR B). There the loopback is the performer: whatever the monitor
//! plays comes back in the microphone, as a person who plays exactly what
//! they hear would. The take cut out for the region must then be the
//! monitor's own region (the target track muted) at a lag of 0, within 1 ms,
//! and the same capture cut at the region's nominal time must not.

use recorder::calibration::{calibrate, measure_once, mix_to_mono, Calibration, RunMeasurement};
use recorder::duplex::{default_pair, play_and_capture, DevicePair};
use recorder::latency::locate;
use recorder::punch::{
    build_monitor, capture_frames, convert, extract, record, Phase, PunchError, Region, Rendered,
};

fn setup() -> DevicePair {
    assert_eq!(
        std::env::var("EDYTLAB_AUDIO_LOOPBACK").as_deref(),
        Ok("1"),
        "set EDYTLAB_AUDIO_LOOPBACK=1 (scripts/audio-loopback.sh does) to run against real audio"
    );
    default_pair().expect("a default output and input device")
}

fn ms(sec: f64) -> f64 {
    sec * 1e3
}

fn describe(label: &str, r: &RunMeasurement) {
    let l: Vec<String> = r
        .latencies_sec
        .iter()
        .map(|s| format!("{:.3}", ms(*s)))
        .collect();
    eprintln!(
        "{label}: [{}] ms, least sure peak {:.0}x",
        l.join(", "),
        r.min_confidence
    );
}

#[test]
#[ignore = "needs a looped-back device pair: scripts/audio-loopback.sh"]
fn calibration_is_steady_and_predicts_a_fresh_run() {
    let pair = setup();
    eprintln!("pair: {}", pair.key());

    let mut runs = Vec::new();
    for n in 1..=3 {
        let r = measure_once(&pair, &|| false).expect("a calibration run");
        describe(&format!("calibration run {n}"), &r);
        runs.push(r);
    }
    let cal = Calibration::from_runs(&runs).expect("three runs that agree");
    eprintln!(
        "calibrated: {:.3} ms, spread {:.3} ms over {} sweeps",
        cal.latency_ms, cal.spread_ms, cal.sweeps_heard
    );
    assert!(cal.spread_ms <= 1.0, "spread {} ms", cal.spread_ms);
    assert!(
        cal.latency_ms > 5.0,
        "a real device has a delay: {} ms",
        cal.latency_ms
    );

    let mut worst_miss: f64 = 0.0;
    let mut least_nominal_miss = f64::MAX;
    for n in 1..=3 {
        let fresh = measure_once(&pair, &|| false).expect("a fresh run");
        describe(&format!("fresh run {n}"), &fresh);
        for l in &fresh.latencies_sec {
            // The sweep landed where `L` + the run's own clocks said (the
            // miss is the sweep's latency less the stored one) ...
            worst_miss = worst_miss.max((ms(*l) - cal.latency_ms).abs());
            // ... and where the nominal splice (no latency) said, it did not.
            least_nominal_miss = least_nominal_miss.min(ms(*l).abs());
        }
    }
    eprintln!(
        "worst prediction miss with the stored latency: {worst_miss:.3} ms; \
         least miss with none: {least_nominal_miss:.3} ms"
    );
    assert!(
        worst_miss <= 1.0,
        "stored latency missed a sweep by {worst_miss} ms"
    );
    assert!(
        least_nominal_miss > 5.0,
        "no-latency splice missed by only {least_nominal_miss} ms"
    );
}

#[test]
#[ignore = "needs a looped-back device pair: scripts/audio-loopback.sh"]
fn the_public_calibrate_agrees_with_its_runs() {
    let pair = setup();
    let mut seen = Vec::new();
    let cal = calibrate(&pair, &|| false, &mut |n, of| seen.push((n, of))).expect("calibrate");
    assert_eq!(seen, vec![(1, 3), (2, 3), (3, 3)]);
    eprintln!(
        "calibrate(): {:.3} ms, spread {:.3} ms",
        cal.latency_ms, cal.spread_ms
    );
    assert!(cal.latency_ms > 5.0 && cal.spread_ms <= 1.0);
}

#[test]
#[ignore = "needs a looped-back device pair: scripts/audio-loopback.sh"]
fn cancel_ends_a_run_promptly() {
    let pair = setup();
    let started = std::time::Instant::now();
    let err = measure_once(&pair, &|| true).expect_err("cancelled before it started");
    assert_eq!(err.to_string(), "cancelled");
    assert!(started.elapsed().as_secs_f64() < 2.0);
}

// ---------------------------------------------------------------------------
// Punch-in: the loopback is the performer.
// ---------------------------------------------------------------------------

/// Broadband noise from a seeded generator (xorshift): `rms` is about the
/// level in full scale. Noise correlates sharply with itself and with
/// nothing else, so a lag shows as a single clear peak.
fn noise(frames: usize, ch: u16, seed: u64, rms: f32) -> Vec<f32> {
    let mut x = seed | 1;
    let mut out = Vec::with_capacity(frames * ch as usize);
    for _ in 0..frames {
        let mut v = 0.0f32;
        // Sum of four uniforms: roughly Gaussian, cheap, deterministic.
        for _ in 0..4 {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            v += (x >> 11) as f32 / (1u64 << 53) as f32 - 0.5;
        }
        for _ in 0..ch {
            out.push(v * rms * 1.7);
        }
    }
    out
}

const REGION: Region = Region {
    start_sec: 1.0,
    end_sec: 2.0,
};
const TOTAL_SEC: f64 = 3.0;

/// The two renders the app would make: the mix as it is (noise A) and the
/// mix with the target muted (noise B).
fn renders(pair: &DevicePair) -> (Rendered, Rendered) {
    let frames = (TOTAL_SEC * f64::from(pair.out_sr)) as usize;
    let r = |seed| Rendered {
        samples: noise(frames, pair.out_ch, seed, 0.15),
        sample_rate: pair.out_sr,
        channels: pair.out_ch,
    };
    (r(0x00A1_0A1A), r(0x00B2_0B2B))
}

/// A render's region as the microphone would hear it (mono, the
/// microphone's rate), less `guard` frames at each end so the reference is
/// wholly inside any take that is a little off.
fn heard(r: &Rendered, pair: &DevicePair, guard: usize) -> Vec<f32> {
    let from = (REGION.start_sec * f64::from(r.sample_rate)) as usize * r.channels as usize;
    let to = (REGION.end_sec * f64::from(r.sample_rate)) as usize * r.channels as usize;
    let at_in = convert(
        &r.samples[from..to],
        r.sample_rate,
        r.channels as usize,
        pair.in_sr,
        1,
    )
    .expect("convert the render");
    at_in[guard..at_in.len() - guard].to_vec()
}

/// Lag, in ms, of `reference` inside the take (positive: the take has it
/// late), and how sure the match is. `None` when it does not match.
fn lag_in_ms(
    reference: &[f32],
    take_samples: &[f32],
    take_ch: u16,
    in_sr: u32,
    guard: usize,
) -> Option<(f64, f32)> {
    let mono = mix_to_mono(take_samples, take_ch);
    let found = locate(reference, &mono, in_sr)?;
    Some((
        (found.position - guard as f64) / f64::from(in_sr) * 1e3,
        found.confidence,
    ))
}

#[test]
#[ignore = "needs a looped-back device pair: scripts/audio-loopback.sh"]
fn a_punched_take_is_the_muted_monitor_at_lag_zero_and_an_uncompensated_splice_is_not() {
    let pair = setup();
    eprintln!("pair: {}", pair.key());
    let cal = calibrate(&pair, &|| false, &mut |_, _| {}).expect("calibrate");
    let latency = cal.latency_ms / 1e3;
    eprintln!(
        "calibrated {:.3} ms (spread {:.3})",
        cal.latency_ms, cal.spread_ms
    );

    let (as_is, muted) = renders(&pair);
    let monitor = build_monitor(&as_is, &muted, REGION, pair.out_sr, pair.out_ch).unwrap();
    let guard = (0.1 * f64::from(pair.in_sr)) as usize;
    let want = heard(&muted, &pair, guard);
    let not_this = heard(&as_is, &pair, guard);

    // 1. The whole pipeline, as the app runs it.
    let phases = std::cell::RefCell::new(Vec::new());
    let take = record(&pair, &monitor, REGION, latency, &|| false, &|p| {
        phases.borrow_mut().push(p.phase);
    })
    .expect("a take");
    assert_eq!(
        take.samples.len(),
        (REGION.len_sec() * f64::from(pair.in_sr)).round() as usize * take.channels as usize
    );
    let (lag, confidence) = lag_in_ms(&want, &take.samples, take.channels, take.sample_rate, guard)
        .expect("the muted monitor is in the take");
    eprintln!("punched take vs the muted monitor: lag {lag:.3} ms, confidence {confidence:.0}x");
    assert!(lag.abs() <= 1.0, "the take is {lag} ms from the monitor");

    // The track was muted in the region: the take is not the as-is mix.
    let other = lag_in_ms(
        &not_this,
        &take.samples,
        take.channels,
        take.sample_rate,
        guard,
    );
    eprintln!("punched take vs the as-is mix: {other:?}");
    assert!(
        other.is_none_or(|(_, c)| c * 3.0 < confidence),
        "the as-is mix is in the take: {other:?}"
    );

    // The strip saw the take go through its three parts, in order.
    let phases = phases.into_inner();
    eprintln!("phases seen: {} polls", phases.len());
    assert_eq!(phases.first(), Some(&Phase::Preroll));
    assert!(phases.contains(&Phase::Recording));
    assert!(
        phases.windows(2).all(|w| w[0] as u8 <= w[1] as u8),
        "{phases:?}"
    );

    // 2. One more capture, cut both ways.
    let frames = capture_frames(TOTAL_SEC, REGION, latency, pair.in_sr);
    let capture = play_and_capture(&monitor, &pair, frames, &|| false).expect("a capture");
    let placed = extract(&capture, REGION, latency).expect("placed");
    let (lag, _) = lag_in_ms(
        &want,
        &placed.samples,
        placed.channels,
        placed.sample_rate,
        guard,
    )
    .expect("placed take matches");
    // The uncompensated splice: the streams' clocks related as they are
    // for the placed take, but no latency, which is what a punch with no
    // calibration would do. It is late by the whole round trip.
    let uncompensated = extract(&capture, REGION, 0.0).expect("cut without the latency");
    let late = lag_in_ms(
        &want,
        &uncompensated.samples,
        uncompensated.channels,
        uncompensated.sample_rate,
        guard,
    );
    // And the plain nominal splice: cut at the region's own seconds of the
    // capture, with no clock model at all. Its error is the round trip
    // less however much earlier the output's frames were issued than the
    // microphone's (a device that fills its buffer ahead), so on a loopback
    // it can cancel to nothing by luck, and it is only reported.
    let mut nominal = capture.clone();
    nominal.out_origin_ns = Some(0);
    nominal.in_origin_ns = Some(0);
    let plain = extract(&nominal, REGION, 0.0).expect("cut at the nominal time");
    let plain_lag = lag_in_ms(
        &want,
        &plain.samples,
        plain.channels,
        plain.sample_rate,
        guard,
    );
    eprintln!(
        "same capture: placed lag {lag:.3} ms; without the latency {late:?}; at the nominal \
         seconds {plain_lag:?}"
    );
    assert!(lag.abs() <= 1.0, "placed take is {lag} ms off");
    let (late_ms, _) = late.expect("a take cut 75 ms early still holds the monitor");
    assert!(
        late_ms.abs() > 5.0,
        "the splice without the latency is only {late_ms} ms off"
    );
    assert!(
        (late_ms - cal.latency_ms).abs() < 2.0,
        "and it is off by the latency: {late_ms} ms against {} ms",
        cal.latency_ms
    );
}

#[test]
#[ignore = "needs a looped-back device pair: scripts/audio-loopback.sh"]
fn a_punch_cancelled_before_it_starts_leaves_no_take() {
    let pair = setup();
    let (as_is, muted) = renders(&pair);
    let monitor = build_monitor(&as_is, &muted, REGION, pair.out_sr, pair.out_ch).unwrap();
    let started = std::time::Instant::now();
    let e = record(&pair, &monitor, REGION, 0.05, &|| true, &|_| {}).expect_err("cancelled");
    assert!(matches!(e, PunchError::Cancelled), "{e}");
    assert!(started.elapsed().as_secs_f64() < 2.0);
}
