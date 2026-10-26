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

use recorder::calibration::{calibrate, measure_once, Calibration, RunMeasurement};
use recorder::duplex::{default_pair, DevicePair};

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
