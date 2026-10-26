//! Measuring a device pair's latency: what is played, how the capture is
//! read, and when the answer is refused (#203).
//!
//! A run plays [`SWEEP_COUNT`] sweeps through the monitor and records the
//! microphone. Each sweep, found in the capture, gives one estimate of the
//! constant `L` of the model in [`crate::latency`]: where it was heard,
//! less where the clock model says a sweep played at that moment would be
//! heard with no latency at all.
//!
//! Two things make the answer trustworthy, and both are refusals rather
//! than adjustments:
//!
//! - **Every sweep must be heard.** A speaker that is off, headphones the
//!   microphone cannot hear, a noisy room: fewer than five found is a
//!   reason the person can act on, not a smaller sample to average.
//! - **They must agree.** The sweeps of a run, and the runs of a
//!   calibration, are within [`MAX_SPREAD_MS`] of each other or the
//!   latency of this device is not one number and nothing here can
//!   compensate it. (Within a run the clock estimate is shared, so only
//!   separate runs can show it moving.)
//!
//! The sweeps are spaced irregularly on purpose. Evenly spaced, a latency
//! of one gap with the first sweep unheard looks exactly like no latency
//! with every sweep heard; with every gap different, the pattern fits one
//! latency only, and a missing sweep is a missing sweep.

use thiserror::Error;

use crate::duplex::{play_and_capture, Capture, DevicePair};
use crate::latency::{locate_all, sweep, Located, SWEEP_SEC};
use crate::RecorderError;

/// How many sweeps one run plays.
pub const SWEEP_COUNT: usize = 5;
/// When each starts on the monitor, in seconds. The gaps (0.45, 0.50,
/// 0.55, 0.60) are all different.
pub const SWEEP_STARTS_SEC: [f64; SWEEP_COUNT] = [0.30, 0.75, 1.25, 1.80, 2.40];
/// The widest disagreement between sweeps that still counts as steady.
pub const MAX_SPREAD_MS: f64 = 1.0;
/// The longest round trip a calibration looks for. Bluetooth output is
/// the long case, at 100 to 300 ms.
pub const MAX_LATENCY_SEC: f64 = 0.8;
/// Quiet after the last sweep's expected arrival, so a late one is not
/// cut off by the end of the capture.
const TAIL_SEC: f64 = 0.2;
/// How near its expected place a found sweep must be to count as that
/// sweep.
const MATCH_TOLERANCE_SEC: f64 = 0.002;
/// A latency a little under zero is the clock estimate's error, not a
/// sound that arrived before it was played.
const MIN_LATENCY_SEC: f64 = -0.05;

#[derive(Debug, Error, PartialEq)]
pub enum CalibrationError {
    #[error(
        "heard {heard} of {expected} test sweeps. Turn the speakers up, or hold the headphones \
         against the microphone, and try again in a quiet room"
    )]
    NotHeard { heard: usize, expected: usize },
    #[error(
        "the measurements disagree by {spread_ms:.1} ms (at most {max} ms is allowed), so this \
         device's delay is not steady enough to compensate. Close other apps that use audio and \
         try again",
        max = MAX_SPREAD_MS
    )]
    Unsteady { spread_ms: f64 },
}

/// What one run found.
#[derive(Debug, Clone, PartialEq)]
pub struct RunMeasurement {
    /// One estimate of the latency per sweep, in seconds, in the order the
    /// sweeps were played.
    pub latencies_sec: Vec<f64>,
    /// The least sure of the sweeps ([`Located::confidence`]).
    pub min_confidence: f32,
}

/// The result of a calibration, ready to store.
#[derive(Debug, Clone, PartialEq)]
pub struct Calibration {
    pub latency_ms: f64,
    pub spread_ms: f64,
    pub sweeps_heard: usize,
    pub runs: usize,
}

/// What the output plays: the sweeps on every channel, then quiet.
pub fn monitor(out_sr: u32, out_ch: u16) -> Vec<f32> {
    let ch = out_ch.max(1) as usize;
    let s = sweep(out_sr);
    let last = SWEEP_STARTS_SEC[SWEEP_COUNT - 1];
    let frames = ((last + SWEEP_SEC + 0.1) * out_sr as f64).ceil() as usize;
    let mut out = vec![0.0f32; frames * ch];
    for start in SWEEP_STARTS_SEC {
        let at = (start * out_sr as f64).round() as usize;
        for (k, v) in s.iter().enumerate() {
            for c in 0..ch {
                out[(at + k) * ch + c] = *v;
            }
        }
    }
    out
}

/// How many input frames to record: the whole monitor, then enough to hear
/// the longest latency looked for.
pub fn capture_frames(in_sr: u32) -> usize {
    let last = SWEEP_STARTS_SEC[SWEEP_COUNT - 1];
    ((last + SWEEP_SEC + MAX_LATENCY_SEC + TAIL_SEC) * in_sr as f64).ceil() as usize
}

/// Interleaved channels averaged to one. A trailing partial frame is
/// dropped.
pub fn mix_to_mono(samples: &[f32], channels: u16) -> Vec<f32> {
    let ch = channels.max(1) as usize;
    samples
        .chunks_exact(ch)
        .map(|f| f.iter().sum::<f32>() / ch as f32)
        .collect()
}

/// Read one run's capture.
///
/// `skew_ns` is the run's `P0 - C0` ([`crate::latency`]). Sweeps are found
/// in the capture and matched to the schedule by the one latency that fits
/// most of them.
pub fn measure_run(
    mono: &[f32],
    in_sr: u32,
    skew_ns: i64,
) -> Result<RunMeasurement, CalibrationError> {
    let hits = locate_all(&sweep(in_sr), mono, in_sr, SWEEP_COUNT * 2);
    let sr = in_sr as f64;
    let skew = skew_ns as f64 * 1e-9;
    let at = |h: &Located| h.position / sr;
    let heard_at = |t: f64| {
        hits.iter()
            .find(|h| (at(h) - t).abs() < MATCH_TOLERANCE_SEC)
    };

    // Every pairing of a found sweep with a scheduled one proposes a
    // latency; the right one is proposed by all of them.
    let mut best: Option<(usize, f64)> = None;
    for h in &hits {
        for start in SWEEP_STARTS_SEC {
            let candidate = at(h) - start - skew;
            if !(MIN_LATENCY_SEC..=MAX_LATENCY_SEC).contains(&candidate) {
                continue;
            }
            let support = SWEEP_STARTS_SEC
                .iter()
                .filter(|s| heard_at(**s + skew + candidate).is_some())
                .count();
            let better = match best {
                None => true,
                Some((n, c)) => support > n || (support == n && candidate < c),
            };
            if better {
                best = Some((support, candidate));
            }
        }
    }
    let Some((_, candidate)) = best else {
        return Err(CalibrationError::NotHeard {
            heard: 0,
            expected: SWEEP_COUNT,
        });
    };

    let mut latencies_sec = Vec::with_capacity(SWEEP_COUNT);
    let mut min_confidence = f32::MAX;
    for start in SWEEP_STARTS_SEC {
        if let Some(h) = heard_at(start + skew + candidate) {
            latencies_sec.push(at(h) - start - skew);
            min_confidence = min_confidence.min(h.confidence);
        }
    }
    if latencies_sec.len() < SWEEP_COUNT {
        return Err(CalibrationError::NotHeard {
            heard: latencies_sec.len(),
            expected: SWEEP_COUNT,
        });
    }
    Ok(RunMeasurement {
        latencies_sec,
        min_confidence,
    })
}

impl Calibration {
    /// Combine runs: the median of every sweep's estimate, refused when
    /// the estimates spread more than [`MAX_SPREAD_MS`].
    pub fn from_runs(runs: &[RunMeasurement]) -> Result<Self, CalibrationError> {
        let mut all: Vec<f64> = runs
            .iter()
            .flat_map(|r| r.latencies_sec.iter().copied())
            .collect();
        if all.is_empty() {
            return Err(CalibrationError::NotHeard {
                heard: 0,
                expected: SWEEP_COUNT,
            });
        }
        all.sort_by(|a, b| a.total_cmp(b));
        let spread_ms = (all[all.len() - 1] - all[0]) * 1e3;
        if spread_ms > MAX_SPREAD_MS + 1e-9 {
            return Err(CalibrationError::Unsteady { spread_ms });
        }
        let mid = all.len() / 2;
        let median = if all.len() % 2 == 1 {
            all[mid]
        } else {
            (all[mid - 1] + all[mid]) / 2.0
        };
        Ok(Self {
            latency_ms: median * 1e3,
            spread_ms,
            sweeps_heard: all.len(),
            runs: runs.len(),
        })
    }
}

/// Why a calibration did not produce a number.
#[derive(Debug, Error)]
pub enum CalibrateError {
    /// The sweeps were not heard, or did not agree. The text is for the
    /// person.
    #[error("{0}")]
    Refused(#[from] CalibrationError),
    #[error("{0}")]
    Device(#[from] RecorderError),
    #[error("the audio device dropped out during the test. Close other apps that use audio and try again")]
    Dropout,
    #[error(
        "the speakers never started playing, so there was nothing to measure. Check that the \
         output is not muted, switched off or in use by another app, and try again"
    )]
    OutputNeverStarted,
    #[error("the microphone gave no timing information, so its delay cannot be measured")]
    NoClock,
    #[error("cancelled")]
    Cancelled,
}

/// One run through the real devices: play the sweeps, record, read them.
/// This is what [`calibrate`] repeats, and what a test of the model on a
/// loopback reads run by run.
pub fn measure_once(
    pair: &DevicePair,
    cancel: &dyn Fn() -> bool,
) -> Result<RunMeasurement, CalibrateError> {
    let played = monitor(pair.out_sr, pair.out_ch);
    let capture = play_and_capture(&played, pair, capture_frames(pair.in_sr), cancel)?;
    read_capture(&capture)
}

/// What a run's capture says, or why it says nothing.
///
/// The microphone always delivers (a capture that fills is one that
/// heard callbacks), so a missing output origin means the speakers never
/// called back past their warm-up, and that is the message, not a vague
/// "no timing".
pub fn read_capture(capture: &Capture) -> Result<RunMeasurement, CalibrateError> {
    if capture.cancelled {
        return Err(CalibrateError::Cancelled);
    }
    if capture.xrun {
        return Err(CalibrateError::Dropout);
    }
    if capture.out_origin_ns.is_none() {
        return Err(CalibrateError::OutputNeverStarted);
    }
    let skew = capture.skew_ns().ok_or(CalibrateError::NoClock)?;
    let mono = mix_to_mono(&capture.samples, capture.in_ch);
    Ok(measure_run(&mono, capture.in_sr, skew)?)
}

/// How many separate runs a calibration makes. Each run estimates the two
/// streams' clocks afresh, so the runs are what show the estimate moving.
pub const RUNS: usize = 3;

/// Measure `pair`: [`RUNS`] runs, combined by [`Calibration::from_runs`].
/// `on_run` is told which run (from 1) is about to start. Stops at the
/// first run that is refused, because a second attempt at the same
/// speakers will not hear more.
pub fn calibrate(
    pair: &DevicePair,
    cancel: &dyn Fn() -> bool,
    on_run: &mut dyn FnMut(usize, usize),
) -> Result<Calibration, CalibrateError> {
    let mut runs = Vec::with_capacity(RUNS);
    for n in 1..=RUNS {
        if cancel() {
            return Err(CalibrateError::Cancelled);
        }
        on_run(n, RUNS);
        runs.push(measure_once(pair, cancel)?);
    }
    Ok(Calibration::from_runs(&runs)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::latency::testkit::{noise, rms};
    use crate::latency::{input_frame_for, sweep};

    /// A made-up room: the monitor's frame 0 is handed to the device
    /// `skew_ns` before the capture's frame 0 begins, and what is played
    /// is heard `latency_sec` later.
    struct Room {
        in_sr: u32,
        skew_ns: i64,
        latency_sec: f64,
        gain: f32,
        noise_rms: f64,
        seed: u64,
        /// Sweeps that never arrive (a muted speaker, a dropout).
        lost: Vec<usize>,
    }

    impl Room {
        fn new(in_sr: u32, skew_ns: i64, latency_sec: f64) -> Self {
            Self {
                in_sr,
                skew_ns,
                latency_sec,
                gain: 1.0,
                noise_rms: 0.0,
                seed: 1,
                lost: vec![],
            }
        }

        fn capture(&self) -> Vec<f32> {
            let mut c = noise(capture_frames(self.in_sr), self.noise_rms, self.seed);
            let s = sweep(self.in_sr);
            for (i, start) in SWEEP_STARTS_SEC.iter().enumerate() {
                if self.lost.contains(&i) {
                    continue;
                }
                let at = input_frame_for(*start, self.skew_ns, self.latency_sec, self.in_sr);
                for (k, v) in s.iter().enumerate() {
                    if let Some(slot) = c.get_mut(at as usize + k) {
                        *slot += v * self.gain;
                    }
                }
            }
            c
        }
    }

    #[test]
    fn the_monitor_carries_the_sweeps_on_every_channel() {
        let m = monitor(48_000, 2);
        let s = sweep(48_000);
        let at = (SWEEP_STARTS_SEC[1] * 48_000.0).round() as usize;
        assert!(m.len() >= (SWEEP_STARTS_SEC[4] * 48_000.0) as usize * 2 + s.len() * 2);
        assert_eq!(m[at * 2], s[0]);
        assert_eq!(m[at * 2 + 1], s[0], "the same sweep on both channels");
        assert!((m[(at + 2_000) * 2] - s[2_000]).abs() < 1e-6);
    }

    #[test]
    fn sweeps_are_spaced_so_that_no_two_gaps_are_alike() {
        // A missing sweep must not be mistaken for a late one: with the
        // gaps all different, the pattern fits one latency only.
        let gaps: Vec<f64> = SWEEP_STARTS_SEC.windows(2).map(|w| w[1] - w[0]).collect();
        for (i, a) in gaps.iter().enumerate() {
            for b in &gaps[i + 1..] {
                assert!((a - b).abs() > 0.02, "{a} and {b} are too alike");
            }
        }
        for g in &gaps {
            assert!(
                *g > crate::latency::SWEEP_SEC * 2.0,
                "room for a sweep and its tail"
            );
        }
    }

    #[test]
    fn the_capture_outlasts_the_last_sweep_by_the_longest_latency_allowed() {
        let n = capture_frames(48_000) as f64 / 48_000.0;
        assert!(n >= SWEEP_STARTS_SEC[4] + 0.1 + MAX_LATENCY_SEC);
    }

    #[test]
    fn channels_are_averaged_to_one() {
        assert_eq!(mix_to_mono(&[1.0, 3.0, -1.0, 1.0], 2), vec![2.0, 0.0]);
        assert_eq!(mix_to_mono(&[0.5, 0.25], 1), vec![0.5, 0.25]);
        // A trailing partial frame is dropped, not misread.
        assert_eq!(mix_to_mono(&[1.0, 1.0, 9.0], 2), vec![1.0]);
    }

    #[test]
    fn measures_a_known_latency_to_a_fraction_of_a_millisecond() {
        for (sr, skew, lat) in [
            (48_000u32, 0i64, 0.0123),
            (48_000, 3_500_000, 0.0420),
            (44_100, -7_250_000, 0.0867),
            (48_000, 12_000_000, 0.3000),
        ] {
            let room = Room::new(sr, skew, lat);
            let m = measure_run(&room.capture(), sr, skew).unwrap();
            assert_eq!(m.latencies_sec.len(), SWEEP_COUNT);
            for l in &m.latencies_sec {
                assert!((l - lat).abs() < 0.0002, "{sr} Hz: measured {l}, was {lat}");
            }
            let c = Calibration::from_runs(&[m]).unwrap();
            assert!((c.latency_ms - lat * 1e3).abs() < 0.2);
            assert!(c.spread_ms < 0.2);
            assert_eq!(c.sweeps_heard, SWEEP_COUNT);
        }
    }

    #[test]
    fn measures_it_through_noise_and_a_quiet_speaker() {
        let mut room = Room::new(48_000, 2_000_000, 0.050);
        room.gain = 0.05;
        room.noise_rms = rms(&sweep(48_000)) * 0.05 * 0.6;
        let m = measure_run(&room.capture(), 48_000, room.skew_ns).unwrap();
        for l in &m.latencies_sec {
            assert!((l - 0.050).abs() < 0.0005);
        }
    }

    /// The point of the whole feature, as a test that fails without it.
    #[test]
    fn a_take_extracted_at_the_measured_latency_is_sample_exact_and_the_nominal_splice_is_not() {
        let in_sr = 48_000u32;
        let (skew_ns, true_latency) = (4_200_000i64, 0.0617);
        let room = Room::new(in_sr, skew_ns, true_latency);

        // Calibrate in this room.
        let m = measure_run(&room.capture(), in_sr, skew_ns).unwrap();
        let measured = Calibration::from_runs(&[m]).unwrap().latency_ms / 1e3;

        // A performer answers the monitor's 10.000 s mark with a click
        // that reaches the microphone one round trip later.
        let mark_sec = 10.0;
        let truth = input_frame_for(mark_sec, skew_ns, true_latency, in_sr);

        let compensated = input_frame_for(mark_sec, skew_ns, measured, in_sr);
        assert!(
            (compensated - truth).abs() <= 1,
            "compensated splice is {} frames off",
            compensated - truth
        );

        // What a splice at the nominal punch point does: no latency, and
        // no clock skew, as if the monitor and the capture started together.
        let nominal = input_frame_for(mark_sec, 0, 0.0, in_sr);
        let late_ms = (truth - nominal) as f64 / in_sr as f64 * 1e3;
        assert!(
            late_ms > 60.0,
            "the nominal splice is only {late_ms} ms off"
        );
    }

    #[test]
    fn a_missing_sweep_is_reported_not_papered_over() {
        let mut room = Room::new(48_000, 0, 0.030);
        room.lost = vec![2];
        assert_eq!(
            measure_run(&room.capture(), 48_000, 0).unwrap_err(),
            CalibrationError::NotHeard {
                heard: 4,
                expected: 5
            }
        );
        room.lost = vec![0, 1, 2, 3, 4];
        assert_eq!(
            measure_run(&room.capture(), 48_000, 0).unwrap_err(),
            CalibrationError::NotHeard {
                heard: 0,
                expected: 5
            }
        );
    }

    /// With the gaps all different, two sweeps heard are enough to say
    /// which two they were: this is a refusal for how many, not a wrong
    /// latency from guessing which.
    #[test]
    fn a_latency_longer_than_a_gap_is_not_confused_with_a_missing_sweep() {
        let mut room = Room::new(48_000, 0, 0.520);
        room.lost = vec![0, 3];
        match measure_run(&room.capture(), 48_000, 0).unwrap_err() {
            CalibrationError::NotHeard { heard, .. } => assert_eq!(heard, 3),
            e => panic!("{e}"),
        }
    }

    #[test]
    fn silence_is_heard_as_nothing() {
        assert_eq!(
            measure_run(&vec![0.0; capture_frames(48_000)], 48_000, 0).unwrap_err(),
            CalibrationError::NotHeard {
                heard: 0,
                expected: 5
            }
        );
    }

    fn run(latencies_ms: [f64; 5]) -> RunMeasurement {
        RunMeasurement {
            latencies_sec: latencies_ms.iter().map(|m| m / 1e3).collect(),
            min_confidence: 50.0,
        }
    }

    #[test]
    fn the_answer_is_the_median_and_the_spread_is_range() {
        let c = Calibration::from_runs(&[run([40.0, 40.2, 39.9, 40.1, 40.05])]).unwrap();
        assert!((c.latency_ms - 40.05).abs() < 1e-9);
        assert!((c.spread_ms - 0.3).abs() < 1e-9);
        assert_eq!(c.runs, 1);
        assert_eq!(c.sweeps_heard, 5);
    }

    #[test]
    fn runs_that_disagree_are_refused_even_if_each_is_steady() {
        // The clock model can be off by more between runs than within one:
        // every sweep of a run shares that run's skew estimate.
        let a = run([40.0, 40.1, 40.0, 40.1, 40.0]);
        let b = run([41.6, 41.7, 41.6, 41.7, 41.6]);
        match Calibration::from_runs(&[a, b]).unwrap_err() {
            CalibrationError::Unsteady { spread_ms } => assert!((spread_ms - 1.7).abs() < 1e-6),
            e => panic!("{e}"),
        }
    }

    #[test]
    fn exactly_the_limit_is_allowed_and_nothing_to_combine_is_refused() {
        assert!(Calibration::from_runs(&[run([40.0, 41.0, 40.5, 40.5, 40.5])]).is_ok());
        assert!(Calibration::from_runs(&[run([40.0, 41.01, 40.5, 40.5, 40.5])]).is_err());
        assert_eq!(
            Calibration::from_runs(&[]).unwrap_err(),
            CalibrationError::NotHeard {
                heard: 0,
                expected: SWEEP_COUNT
            }
        );
    }

    fn capture(out_origin_ns: Option<i64>, in_origin_ns: Option<i64>, xrun: bool) -> Capture {
        Capture {
            samples: vec![0.0; capture_frames(48_000)],
            in_sr: 48_000,
            in_ch: 1,
            out_origin_ns,
            in_origin_ns,
            xrun,
            cancelled: false,
        }
    }

    #[test]
    fn speakers_that_never_start_are_named_not_blamed_on_missing_timing() {
        let e = read_capture(&capture(None, Some(0), false)).unwrap_err();
        assert!(matches!(e, CalibrateError::OutputNeverStarted), "{e:?}");
        let text = e.to_string();
        assert!(text.contains("speakers never started playing"), "{text}");
        assert!(!text.contains("timing information"), "{text}");
    }

    #[test]
    fn a_microphone_with_no_clock_still_says_so() {
        let e = read_capture(&capture(Some(0), None, false)).unwrap_err();
        assert!(matches!(e, CalibrateError::NoClock), "{e:?}");
    }

    #[test]
    fn a_dropout_is_reported_before_anything_else_about_the_clocks() {
        let e = read_capture(&capture(None, None, true)).unwrap_err();
        assert!(matches!(e, CalibrateError::Dropout), "{e:?}");
    }
}
