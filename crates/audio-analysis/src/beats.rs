//! Beat tracking — given a tempo estimate, place a periodic beat grid
//! and snap each predicted beat to the nearest detected onset within a
//! ±50 ms window.
//!
//! Downbeat detection is a Phase-2 stub: every Nth beat starting at
//! beat 0 (assume 4/4 unless told otherwise).

use crate::bpm::{pick_onsets, spectral_flux};
use crate::{Error, Result};

/// Detect beat times (in seconds) for `samples_mono` given an estimated
/// `bpm`. Returns a monotonically increasing sequence covering most of
/// the audio.
///
/// Algorithm:
/// 1. Recompute onsets via spectral flux (cheap; `bpm.rs` would compute
///    them anyway and Phase 2 doesn't yet share the intermediate).
/// 2. Anchor on the strongest onset in the first 4 seconds.
/// 3. Step forward in 60/bpm increments, snapping each predicted beat
///    to the nearest onset within ±50 ms.
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

    let flux = spectral_flux(samples_mono, sample_rate)?;
    let onsets = pick_onsets(&flux, sample_rate);
    if onsets.is_empty() {
        return Ok(Vec::new());
    }

    // Anchor: strongest onset in the first 4 seconds. Strength = flux
    // value at the onset's frame.
    let hop_seconds = crate::bpm::HOP as f32 / sample_rate as f32;
    let early = onsets
        .iter()
        .copied()
        .filter(|t| *t < 4.0)
        .collect::<Vec<_>>();
    let pool = if early.is_empty() {
        &onsets[..]
    } else {
        &early
    };
    let mut anchor = pool[0];
    let mut anchor_flux = 0.0f32;
    for &t in pool {
        let frame = (t / hop_seconds) as usize;
        let v = *flux.get(frame).unwrap_or(&0.0);
        if v > anchor_flux {
            anchor_flux = v;
            anchor = t;
        }
    }

    let beat_period = 60.0 / bpm;
    let snap_window = 0.050f32; // ±50 ms

    let mut beats = Vec::new();
    let mut predicted = anchor;
    while predicted <= duration_s + snap_window {
        // Find the onset closest to `predicted` within the snap window.
        let snapped = onsets
            .iter()
            .copied()
            .filter(|t| (t - predicted).abs() <= snap_window)
            .min_by(|a, b| {
                (a - predicted)
                    .abs()
                    .partial_cmp(&(b - predicted).abs())
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .unwrap_or(predicted);
        // Guard monotonic order.
        if let Some(&last) = beats.last() {
            if snapped <= last {
                predicted += beat_period;
                continue;
            }
        }
        beats.push(snapped);
        predicted = snapped + beat_period;
    }

    // Also extend backward toward zero so a later anchor still gives
    // beats covering the start.
    let mut prefix = Vec::new();
    let mut t = anchor - beat_period;
    while t > 0.0 {
        let snapped = onsets
            .iter()
            .copied()
            .filter(|tt| (tt - t).abs() <= snap_window)
            .min_by(|a, b| {
                (a - t)
                    .abs()
                    .partial_cmp(&(b - t).abs())
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .unwrap_or(t);
        if snapped > 0.0 {
            prefix.push(snapped);
        }
        t -= beat_period;
    }
    prefix.reverse();
    prefix.extend(beats);
    Ok(prefix)
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
