//! Beat tracking: lay the beat grid at the chosen tempo, on the strongest
//! onsets of the curve the tempo was read from.
//!
//! [`crate::bpm`] finds the pulse (tempo and phase) on the 30–150 Hz curve
//! when the track has a kick, so the grid lands on the kicks and not on the
//! hats between them (#696). Here each beat of that grid is then nudged to the
//! strongest onset of the same curve within a short window, and the grid
//! follows the nudge a little (a phase-locked loop), so a drummer who pushes
//! the beat by 20 ms is followed and a missed beat does not throw the grid.
//!
//! Downbeat detection is a Phase-2 stub: every Nth beat starting at beat 0
//! (assume 4/4 unless told otherwise).

use crate::bpm::{prepare, pulse_at, Basis, Prepared, Pulse};
use crate::onset::{novelty, Novelty};
use crate::{Error, Result};

/// How far a beat may be pulled from the grid, seconds.
const SNAP_WINDOW_S: f32 = 0.060;
/// ...and as a share of the beat period, whichever is less.
const SNAP_WINDOW_PERIOD: f32 = 0.2;
/// How much of a beat's offset the next predicted beat takes up.
const LOOP_GAIN: f32 = 0.3;
/// A beat is only pulled to an onset at least this strong (the curve is
/// scaled so a strong onset is 1.0).
const SNAP_MIN_STRENGTH: f32 = 0.15;

/// Detect beat times (in seconds) for `samples_mono` given an estimated
/// `bpm`. Returns a monotonically increasing sequence covering the audio.
pub fn detect_beats(samples_mono: &[f32], sample_rate: u32, bpm: f32) -> Result<Vec<f32>> {
    if !(20.0..=400.0).contains(&bpm) {
        return Err(Error::Analysis(format!(
            "bpm out of range for beat tracking: {bpm}"
        )));
    }
    if sample_rate == 0 {
        return Err(Error::Analysis("sample_rate is zero".into()));
    }
    let duration_s = samples_mono.len() as f32 / sample_rate as f32;
    if duration_s < 0.5 {
        return Ok(Vec::new());
    }
    let nov = novelty(samples_mono, sample_rate)?;
    let pulse = pulse_at(&nov, bpm);
    Ok(track(&nov, &pulse, duration_s))
}

/// The beat grid for a pulse already found.
pub(crate) fn track(nov: &Novelty, pulse: &Pulse, duration_s: f32) -> Vec<f32> {
    let prep: Prepared = match pulse.basis {
        Basis::Low => prepare(&nov.low),
        Basis::Full => prepare(&nov.full),
    };
    let n = prep.env.len();
    let period = pulse.period_frames;
    let window = ((SNAP_WINDOW_S / nov.hop_s).min(period * SNAP_WINDOW_PERIOD)).max(1.0);

    let mut beats: Vec<f32> = Vec::new();
    let mut nominal = pulse.phase_frames;
    // Back up to the first grid point at or after time zero (that can be a
    // frame before the first window: a kick on the very first sample).
    while nov.time_of(nominal - period) >= 0.0 {
        nominal -= period;
    }
    while nov.time_of(nominal) <= duration_s {
        let (frame, shift) = snap(&prep.env, n, nominal, window);
        let t = nov.time_of(frame);
        if t >= 0.0 && beats.last().is_none_or(|&last| t > last) {
            beats.push(t);
        }
        nominal += period + LOOP_GAIN * shift;
    }
    beats
}

/// The strongest onset within `window` frames of `nominal`, with the peak
/// refined to a fraction of a frame, and its offset from `nominal`. If there
/// is none strong enough the grid point stands.
fn snap(env: &[f32], n: usize, nominal: f32, window: f32) -> (f32, f32) {
    let lo = (nominal - window).ceil().max(0.0) as usize;
    let hi = ((nominal + window).floor() as isize).min(n as isize - 1);
    if hi < lo as isize {
        return (nominal, 0.0);
    }
    let hi = hi as usize;
    let mut best = lo;
    for i in lo..=hi {
        // Prefer the stronger onset; between equals, the closer to the grid.
        let better = env[i] > env[best]
            || (env[i] == env[best] && (i as f32 - nominal).abs() < (best as f32 - nominal).abs());
        if better {
            best = i;
        }
    }
    if env[best] < SNAP_MIN_STRENGTH {
        return (nominal, 0.0);
    }
    // Parabolic interpolation of the peak.
    let frame = if best > 0 && best + 1 < n {
        let (a, b, c) = (env[best - 1], env[best], env[best + 1]);
        let denom = a - 2.0 * b + c;
        if denom.abs() > 1e-6 {
            best as f32 + (0.5 * (a - c) / denom).clamp(-0.5, 0.5)
        } else {
            best as f32
        }
    } else {
        best as f32
    };
    (frame, frame - nominal)
}

/// Pick downbeats from a beat grid. Phase-2 stub: every `time_signature`
/// beats starting at beat 0. `time_signature` is the numerator (e.g.
/// `4` for 4/4).
pub fn detect_downbeats(beats: &[f32], time_signature: u32) -> Vec<f32> {
    if time_signature == 0 {
        return Vec::new();
    }
    beats
        .iter()
        .enumerate()
        .filter(|(i, _)| (*i as u32).is_multiple_of(time_signature))
        .map(|(_, &t)| t)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn click_track(sample_rate: u32, duration_s: f32, bpm: f32) -> Vec<f32> {
        let n = (duration_s * sample_rate as f32) as usize;
        let period = 60.0 / bpm;
        let click_len = (sample_rate as f32 * 0.01) as usize;
        let mut out = vec![0.0f32; n];
        let mut t = 0.0f32;
        while (t * sample_rate as f32) as usize + click_len < n {
            let start = (t * sample_rate as f32) as usize;
            for i in 0..click_len {
                let env = (1.0 - i as f32 / click_len as f32).max(0.0);
                let s = (i as f32 * 0.5).sin() * env;
                out[start + i] = s;
            }
            t += period;
        }
        out
    }

    #[test]
    fn beats_monotonic_and_periodic_4_4() {
        let sr = 44_100;
        let bpm = 120.0;
        let click = click_track(sr, 8.0, bpm);
        let beats = detect_beats(&click, sr, bpm).expect("ok");
        let n = beats.len();
        assert!(n >= 8, "expected ~16 beats over 8s @ 120 BPM, got {n}");
        // Monotonic.
        for w in beats.windows(2) {
            let (a, b) = (w[0], w[1]);
            assert!(b > a, "beats not monotonic: {a} -> {b}");
        }
        // CV < 5% (acceptance criterion #3).
        let intervals: Vec<f32> = beats.windows(2).map(|w| w[1] - w[0]).collect();
        let mean = intervals.iter().sum::<f32>() / intervals.len() as f32;
        let var =
            intervals.iter().map(|x| (x - mean).powi(2)).sum::<f32>() / intervals.len() as f32;
        let cv = var.sqrt() / mean;
        assert!(cv < 0.05, "interval CV {cv} not < 5%");
    }

    #[test]
    fn downbeats_every_fourth() {
        let beats = vec![0.0, 0.5, 1.0, 1.5, 2.0, 2.5, 3.0, 3.5];
        let dbs = detect_downbeats(&beats, 4);
        assert_eq!(dbs, vec![0.0, 2.0]);
    }

    #[test]
    fn downbeats_zero_sig_empty() {
        let dbs = detect_downbeats(&[0.0, 0.5], 0);
        assert!(dbs.is_empty());
    }
}
