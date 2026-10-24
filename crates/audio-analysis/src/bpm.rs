//! Tempo estimation: which pulse is the beat, and how sure are we.
//!
//! ## Algorithm
//!
//! 1. [`crate::onset::novelty`] makes two onset-strength curves: the whole
//!    spectrum and the 30–150 Hz band (the kick).
//! 2. For every candidate tempo from 50 to 220 BPM (0.5 BPM apart) and every
//!    phase of its pulse train, a **comb** is laid on the curve: its teeth on
//!    the beats, and a probe halfway between each pair. The comb's score is
//!    the mean strength under the teeth minus half the mean strength under
//!    the probes. A real beat has onsets on it and little between; a pulse
//!    at twice the beat has half its teeth in silence (mean halves), and a
//!    pulse at half the beat has its probes on the missing beats (penalty).
//!    That is what separates a tempo from its double and its half, the
//!    octave error of #696, instead of a histogram of gaps, which counts
//!    every hat as a beat.
//! 3. The **low band** decides when the track has one (it has a kick, and
//!    the comb on it is clearly periodic); the whole spectrum decides when it
//!    does not (a click track, a voice, a guitar). Hats and shakers sit
//!    between the beats and raise the whole-spectrum flux more than any
//!    drum, so using the whole spectrum for a track with a kick reads the
//!    off-beats as the beat.
//! 4. A weak prior, a log-Gaussian over tempo centred on 120 BPM one octave
//!    wide (the tempo-period weighting of D. Ellis, "Beat Tracking by Dynamic
//!    Programming", J. New Music Research 36(1), 2007, §2.2), breaks ties
//!    that the comb leaves. It cannot overrule a clear comb: 70 against 140
//!    differs by 11% in prior and 2x in comb score.
//! 5. The winner's half and double, when they are in range, are reported with
//!    the share of the evidence they got, so a caller (or the model) can see
//!    that 70 and 140 were both live and ask.
//!
//! Where it can still be wrong: music with no steady pulse, and the genuine
//! half-time ambiguity (a kick on 1 and 3 only reads as the slower tempo; the
//! double is then in `alternatives`).

use serde::{Deserialize, Serialize};

use crate::onset::{novelty, percentile, Novelty};
use crate::{Error, Result};

/// Slowest tempo considered, BPM.
pub const MIN_BPM: f32 = 50.0;
/// Fastest tempo considered, BPM.
pub const MAX_BPM: f32 = 220.0;
/// Spacing of the coarse search, BPM.
const GRID_STEP: f32 = 0.5;
/// How much the probes between the teeth count against a comb.
const PROBE_WEIGHT: f32 = 0.5;
/// The tempo the prior is centred on, BPM.
const PRIOR_CENTRE_BPM: f32 = 120.0;
/// The prior's width, octaves.
const PRIOR_SIGMA_OCTAVES: f32 = 1.0;
/// The low band is used when a strong onset in it is at least this strong, in
/// log-flux units (see `Novelty::low_level`)...
const LOW_MIN_LEVEL: f32 = 3.0;
/// ...and its best comb scores at least this (a steady kick scores 0.7–1.0).
const LOW_MIN_COMB: f32 = 0.30;
/// The whole-spectrum curve must have onsets at least this strong, in
/// log-flux units (see `Novelty::full_level`), for there to be a pulse. Drums
/// and clicks measure 190–850 on the synthetic signals; a synthetic pad with
/// its edges, 0.7–3.7. A heuristic: a very quiet recording of real drums is
/// not measured here.
const MIN_ONSET_LEVEL: f32 = 10.0;
/// A comb score at or above this is a clear pulse; confidence scales to it.
const CLEAR_PULSE: f32 = 0.5;

/// How an alternative tempo relates to the chosen one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TempoRelation {
    /// Half the chosen tempo.
    Half,
    /// Twice the chosen tempo.
    Double,
}

impl TempoRelation {
    /// The word the tool puts in its result.
    pub fn as_str(self) -> &'static str {
        match self {
            TempoRelation::Half => "half",
            TempoRelation::Double => "double",
        }
    }
}

/// Another reading of the same pulse.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TempoAlternative {
    pub bpm: f32,
    pub relation: TempoRelation,
    /// This reading's share of the evidence, `0..=1` (scaled down when the
    /// pulse itself is weak). The chosen tempo's `confidence` is on the same
    /// scale, so the two are comparable.
    pub confidence: f32,
}

/// The chosen tempo, how sure we are, and the octave alternatives.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tempo {
    pub bpm: f32,
    /// `0..=1`: the chosen tempo's share of the evidence among itself and its
    /// half and double, times how clear the pulse is. Below about 0.5 the
    /// choice between them is a coin toss.
    pub confidence: f32,
    /// The half and the double of `bpm` that are inside 50–220, with their
    /// confidences.
    pub alternatives: Vec<TempoAlternative>,
}

/// Which curve the beat is read from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Basis {
    /// 30–150 Hz: the track has a kick.
    Low,
    /// The whole spectrum.
    Full,
}

/// The pulse under the music: tempo plus where the beats fall.
pub(crate) struct Pulse {
    pub tempo: Tempo,
    /// Frames of the novelty curve between beats.
    pub period_frames: f32,
    /// Frame index of one beat (the one with the earliest position at or
    /// after frame 0).
    pub phase_frames: f32,
    pub basis: Basis,
}

/// Estimate the tempo of a mono audio buffer in beats per minute.
///
/// Returns `Err(Error::Analysis(_))` if the input is too short or has too
/// few onsets to find a pulse.
pub fn estimate_bpm(samples_mono: &[f32], sample_rate: u32) -> Result<f32> {
    Ok(estimate_tempo(samples_mono, sample_rate)?.bpm)
}

/// Like [`estimate_bpm`], with the confidence and the octave alternatives.
pub fn estimate_tempo(samples_mono: &[f32], sample_rate: u32) -> Result<Tempo> {
    check_length(samples_mono, sample_rate)?;
    let nov = novelty(samples_mono, sample_rate)?;
    match estimate(&nov)? {
        Some(pulse) => Ok(pulse.tempo),
        None => Err(Error::Analysis(
            "too few onsets for BPM estimation: no steady pulse found".into(),
        )),
    }
}

pub(crate) fn check_length(samples_mono: &[f32], sample_rate: u32) -> Result<()> {
    if sample_rate == 0 {
        return Err(Error::Analysis("sample_rate is zero".into()));
    }
    let min_required = sample_rate as usize / 2; // need at least ~0.5 s
    if samples_mono.len() < min_required {
        return Err(Error::Analysis(format!(
            "audio too short for BPM estimation: {} samples at {} Hz",
            samples_mono.len(),
            sample_rate
        )));
    }
    Ok(())
}

/// A novelty curve scaled so a typical strong onset is 1.0, clipped there,
/// with its 3-frame running maximum (the tolerance of a comb tooth).
pub(crate) struct Prepared {
    pub env: Vec<f32>,
    pub max3: Vec<f32>,
}

pub(crate) fn prepare(raw: &[f32]) -> Prepared {
    // 99th percentile of the non-zero values: a strong onset, not the one
    // loudest transient in the file.
    let level = percentile(raw, 0.99);
    let env: Vec<f32> = if level > 0.0 {
        raw.iter().map(|v| (v / level).min(1.0)).collect()
    } else {
        vec![0.0; raw.len()]
    };
    let max3 = (0..env.len())
        .map(|i| {
            let lo = i.saturating_sub(1);
            let hi = (i + 1).min(env.len() - 1);
            env[lo..=hi].iter().copied().fold(0.0, f32::max)
        })
        .collect();
    Prepared { env, max3 }
}

/// Comb score of a pulse of `period` frames starting at `phase`: mean under
/// the teeth, minus [`PROBE_WEIGHT`] times the mean under the probes.
fn comb(max3: &[f32], period: f32, phase: f32) -> f32 {
    let n = max3.len();
    let (mut on, mut n_on, mut mid, mut n_mid) = (0.0f32, 0u32, 0.0f32, 0u32);
    let mut k = 0.0f32;
    loop {
        let pos = phase + k * period;
        let i = pos.round() as usize;
        if i >= n {
            break;
        }
        on += max3[i];
        n_on += 1;
        let j = (pos + period / 2.0).round() as usize;
        if j < n {
            mid += max3[j];
            n_mid += 1;
        }
        k += 1.0;
    }
    if n_on == 0 {
        return 0.0;
    }
    let probe = if n_mid > 0 { mid / n_mid as f32 } else { 0.0 };
    on / n_on as f32 - PROBE_WEIGHT * probe
}

/// Best comb score over the phases of a pulse of `period` frames, and the
/// phase that gave it. Phases are `phase_step` frames apart.
fn best_phase(max3: &[f32], period: f32, phase_step: f32) -> (f32, f32) {
    let mut best = (f32::NEG_INFINITY, 0.0f32);
    let mut ph = 0.0f32;
    while ph < period {
        let s = comb(max3, period, ph);
        if s > best.0 {
            best = (s, ph);
        }
        ph += phase_step;
    }
    best
}

fn period_frames(bpm: f32, hop_s: f32) -> f32 {
    60.0 / (bpm * hop_s)
}

/// The log-Gaussian tempo prior, `0..=1`.
fn prior(bpm: f32) -> f32 {
    let octaves = (bpm / PRIOR_CENTRE_BPM).log2() / PRIOR_SIGMA_OCTAVES;
    (-0.5 * octaves * octaves).exp()
}

/// Comb scores for a curve across the whole tempo grid.
struct Curve {
    bpms: Vec<f32>,
    /// Best-phase comb score at each tempo, without the prior.
    raw: Vec<f32>,
}

fn curve(prep: &Prepared, hop_s: f32) -> Curve {
    let n = ((MAX_BPM - MIN_BPM) / GRID_STEP) as usize + 1;
    let bpms: Vec<f32> = (0..n).map(|i| MIN_BPM + i as f32 * GRID_STEP).collect();
    let raw = bpms
        .iter()
        .map(|&b| best_phase(&prep.max3, period_frames(b, hop_s), 1.0).0)
        .collect();
    Curve { bpms, raw }
}

impl Curve {
    fn best_raw(&self) -> f32 {
        self.raw.iter().copied().fold(f32::NEG_INFINITY, f32::max)
    }

    /// Index of the best prior-weighted score.
    fn best_weighted(&self) -> usize {
        let mut best = 0;
        let mut best_s = f32::NEG_INFINITY;
        for (i, (&b, &r)) in self.bpms.iter().zip(&self.raw).enumerate() {
            let s = r.max(0.0) * prior(b);
            if s > best_s {
                best_s = s;
                best = i;
            }
        }
        best
    }

    /// Best prior-weighted score within ±2% of `bpm`, if `bpm` is in range.
    fn weighted_near(&self, bpm: f32) -> Option<f32> {
        // A little slack: the half of 99.9 is 49.95, and that is 50.
        if !(MIN_BPM * 0.98..=MAX_BPM * 1.02).contains(&bpm) {
            return None;
        }
        self.bpms
            .iter()
            .zip(&self.raw)
            .filter(|(b, _)| (**b - bpm).abs() <= bpm * 0.02)
            .map(|(b, r)| r.max(0.0) * prior(*b))
            .fold(None, |acc: Option<f32>, s| {
                Some(acc.map_or(s, |a| a.max(s)))
            })
    }
}

/// Number of separate onsets above `threshold` in `env`.
fn count_onsets(env: &[f32], threshold: f32) -> usize {
    (1..env.len().saturating_sub(1))
        .filter(|&i| env[i] >= threshold && env[i] > env[i - 1] && env[i] >= env[i + 1])
        .count()
}

/// Pick the curve the beat is read from: the low band when it has a kick
/// worth following, else the whole spectrum.
fn choose_basis(nov: &Novelty, low: &Curve) -> Basis {
    if nov.low_level >= LOW_MIN_LEVEL && low.best_raw() >= LOW_MIN_COMB {
        Basis::Low
    } else {
        Basis::Full
    }
}

/// Estimate the pulse from the onset curves. `None` when there are too few
/// onsets to speak of a pulse (a drone, a pad, silence).
pub(crate) fn estimate(nov: &Novelty) -> Result<Option<Pulse>> {
    let low_prep = prepare(&nov.low);
    let full_prep = prepare(&nov.full);
    let low_curve = curve(&low_prep, nov.hop_s);
    let basis = choose_basis(nov, &low_curve);
    let (prep, chosen_curve) = match basis {
        Basis::Low => (&low_prep, low_curve),
        Basis::Full => (&full_prep, curve(&full_prep, nov.hop_s)),
    };

    // Curves are scaled to their own strongest onsets, so on a pad or a drone
    // the rounding noise would be scaled up to look like drums. A floor in
    // absolute terms keeps it out.
    if nov.full_level < MIN_ONSET_LEVEL || count_onsets(&prep.env, 0.3) < 4 {
        return Ok(None);
    }

    let coarse = chosen_curve.best_weighted();
    let bpm = refine(&prep.max3, nov.hop_s, chosen_curve.bpms[coarse]);
    let (raw_best, phase) = best_phase(&prep.max3, period_frames(bpm, nov.hop_s), 0.25);

    // Confidence: the chosen reading's share of the evidence among itself and
    // its in-range half and double, times how clear the pulse is.
    let chosen_w = raw_best.max(0.0) * prior(bpm);
    let half = chosen_curve.weighted_near(bpm / 2.0);
    let double = chosen_curve.weighted_near(bpm * 2.0);
    let total = chosen_w + half.unwrap_or(0.0) + double.unwrap_or(0.0);
    let clarity = (raw_best / CLEAR_PULSE).clamp(0.0, 1.0);
    let share = |w: f32| {
        if total > 0.0 {
            w / total * clarity
        } else {
            0.0
        }
    };

    let mut alternatives = Vec::new();
    if let Some(w) = half {
        alternatives.push(TempoAlternative {
            bpm: round1(bpm / 2.0),
            relation: TempoRelation::Half,
            confidence: round2(share(w)),
        });
    }
    if let Some(w) = double {
        alternatives.push(TempoAlternative {
            bpm: round1(bpm * 2.0),
            relation: TempoRelation::Double,
            confidence: round2(share(w)),
        });
    }

    Ok(Some(Pulse {
        tempo: Tempo {
            bpm: round1(bpm),
            confidence: round2(share(chosen_w)),
            alternatives,
        },
        period_frames: period_frames(bpm, nov.hop_s),
        phase_frames: phase,
        basis,
    }))
}

/// The pulse at a tempo the caller already has: the basis and the phase,
/// not the tempo.
pub(crate) fn pulse_at(nov: &Novelty, bpm: f32) -> Pulse {
    let low_prep = prepare(&nov.low);
    let low_curve = curve(&low_prep, nov.hop_s);
    let basis = choose_basis(nov, &low_curve);
    let prep = match basis {
        Basis::Low => low_prep,
        Basis::Full => prepare(&nov.full),
    };
    let period = period_frames(bpm, nov.hop_s);
    let (score, phase) = best_phase(&prep.max3, period, 0.25);
    Pulse {
        tempo: Tempo {
            bpm,
            confidence: round2((score / CLEAR_PULSE).clamp(0.0, 1.0)),
            alternatives: Vec::new(),
        },
        period_frames: period,
        phase_frames: phase,
        basis,
    }
}

/// Narrow a coarse tempo to 0.1 BPM: the middle of the plateau of tempos
/// whose best comb is within a hair of the maximum.
fn refine(max3: &[f32], hop_s: f32, coarse: f32) -> f32 {
    let lo = (coarse - GRID_STEP * 2.0).max(MIN_BPM);
    let hi = (coarse + GRID_STEP * 2.0).min(MAX_BPM);
    let steps = ((hi - lo) / 0.1).round() as usize;
    let scored: Vec<(f32, f32)> = (0..=steps)
        .map(|i| {
            let b = lo + i as f32 * 0.1;
            (b, best_phase(max3, period_frames(b, hop_s), 1.0).0)
        })
        .collect();
    let top = scored
        .iter()
        .map(|(_, s)| *s)
        .fold(f32::NEG_INFINITY, f32::max);
    let plateau: Vec<f32> = scored
        .iter()
        .filter(|(_, s)| *s >= top - 1e-3)
        .map(|(b, _)| *b)
        .collect();
    plateau.iter().sum::<f32>() / plateau.len() as f32
}

fn round1(x: f32) -> f32 {
    (x * 10.0).round() / 10.0
}

fn round2(x: f32) -> f32 {
    (x * 100.0).round() / 100.0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Synthetic click track: short loud impulses every `period_s` seconds.
    fn click_track(sample_rate: u32, duration_s: f32, bpm: f32) -> Vec<f32> {
        let n = (duration_s * sample_rate as f32) as usize;
        let period = 60.0 / bpm;
        let click_len = (sample_rate as f32 * 0.01) as usize; // 10 ms click
        let mut out = vec![0.0f32; n];
        let mut t = 0.0f32;
        while (t * sample_rate as f32) as usize + click_len < n {
            let start = (t * sample_rate as f32) as usize;
            for i in 0..click_len {
                // Decaying sinusoidal pop, broadband-ish.
                let env = (1.0 - i as f32 / click_len as f32).max(0.0);
                let s = (i as f32 * 0.5).sin() * env;
                out[start + i] = s;
            }
            t += period;
        }
        out
    }

    #[test]
    fn click_track_120_bpm() {
        let sr = 44_100;
        let click = click_track(sr, 8.0, 120.0);
        let bpm = estimate_bpm(&click, sr).expect("estimate ok");
        assert!((bpm - 120.0).abs() <= 1.0, "expected ~120 BPM, got {bpm}");
    }

    #[test]
    fn click_track_90_bpm() {
        let sr = 44_100;
        let click = click_track(sr, 10.0, 90.0);
        let bpm = estimate_bpm(&click, sr).expect("estimate ok");
        assert!((bpm - 90.0).abs() <= 1.0, "expected ~90 BPM, got {bpm}");
    }

    #[test]
    fn a_clean_click_track_is_confident_and_lists_its_octaves() {
        let sr = 44_100;
        let t = estimate_tempo(&click_track(sr, 12.0, 100.0), sr).expect("ok");
        assert!((t.bpm - 100.0).abs() <= 1.0, "{t:?}");
        assert!(t.confidence >= 0.5, "{t:?}");
        // 50 and 200 are both in range, and both are weaker readings.
        let half = t
            .alternatives
            .iter()
            .find(|a| a.relation == TempoRelation::Half);
        let double = t
            .alternatives
            .iter()
            .find(|a| a.relation == TempoRelation::Double);
        let (half, double) = (half.expect("half"), double.expect("double"));
        assert!((half.bpm - 50.0).abs() <= 1.0 && (double.bpm - 200.0).abs() <= 1.0);
        assert!(half.confidence < t.confidence && double.confidence < t.confidence);
    }

    #[test]
    fn alternatives_outside_the_range_are_left_out() {
        let sr = 44_100;
        let t = estimate_tempo(&click_track(sr, 10.0, 180.0), sr).expect("ok");
        assert!((t.bpm - 180.0).abs() <= 1.5, "{t:?}");
        assert!(
            t.alternatives
                .iter()
                .all(|a| a.relation == TempoRelation::Half),
            "360 is out of range: {t:?}"
        );
    }

    #[test]
    fn prior_peaks_at_120_and_is_gentle() {
        assert!((prior(120.0) - 1.0).abs() < 1e-6);
        assert!(prior(70.0) > 0.7 && prior(140.0) > 0.95);
        assert!(prior(60.0) > 0.5);
    }

    #[test]
    fn empty_input_errors() {
        let res = estimate_bpm(&[], 44_100);
        assert!(res.is_err());
    }

    #[test]
    fn zero_sample_rate_errors() {
        let res = estimate_bpm(&[0.1; 4096], 0);
        assert!(res.is_err());
    }

    #[test]
    fn silence_errors_rather_than_guessing() {
        assert!(estimate_bpm(&vec![0.0; 44_100 * 4], 44_100).is_err());
    }

    #[test]
    fn a_sustained_tone_has_no_pulse() {
        let tone: Vec<f32> = (0..44_100 * 6)
            .map(|i| 0.5 * (std::f32::consts::TAU * 220.0 * i as f32 / 44_100.0).sin())
            .collect();
        assert!(estimate_bpm(&tone, 44_100).is_err());
    }
}
