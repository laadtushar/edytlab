//! Key detection via 12-bin chroma + Krumhansl–Kessler key profiles.
//!
//! ## Why pure-Rust here
//!
//! The plan describes a three-rung selection ladder (ONNX → pure-Rust
//! Krumhansl–Schmuckler → Python sidecar). This module is rung 2,
//! locked in for this PR because the ONNX rung needs an out-of-band
//! model gate (see also `ml-demucs` in M18) and the sidecar rung is
//! incompatible with the sandbox.
//!
//! ## Algorithm
//!
//! 1. STFT (window = 4096, hop = 2048) — a coarser window than BPM so
//!    pitch class energy is well-resolved at the bottom of the keyboard.
//! 2. Per-frame: bin spectrum energy by pitch class (`12 * log2(f / C0)
//!    mod 12`), accumulate over the whole track.
//! 3. Normalise the chroma to unit sum.
//! 4. Score against 24 key profiles (12 major + 12 minor) by Pearson
//!    correlation; return the highest-correlation key.
//!
//! Profile constants are the canonical Krumhansl & Kessler 1982 study
//! values, widely reproduced in MIR literature.

use realfft::num_complex::Complex;
use realfft::RealFftPlanner;

use crate::{Error, Result};

const FFT_SIZE: usize = 4096;
const HOP: usize = 2048;

/// Krumhansl–Kessler 1982 major key profile (C major).
pub const KRUMHANSL_MAJOR: [f32; 12] = [
    6.35, 2.23, 3.48, 2.33, 4.38, 4.09, 2.52, 5.19, 2.39, 3.66, 2.29, 2.88,
];
/// Krumhansl–Kessler 1982 minor key profile (C minor).
pub const KRUMHANSL_MINOR: [f32; 12] = [
    6.33, 2.68, 3.52, 5.38, 2.60, 3.53, 2.54, 4.75, 3.98, 2.69, 3.34, 3.17,
];

const PITCH_NAMES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];

/// Estimate the key of a mono audio buffer. Returns `"<root> <mode>"`,
/// e.g. `"C major"`, `"F# minor"`.
pub fn estimate_key(samples_mono: &[f32], sample_rate: u32) -> Result<String> {
    Ok(estimate_key_with_confidence(samples_mono, sample_rate)?.0)
}

/// Like [`estimate_key`] but also returns a Pearson correlation as a
/// confidence value in `[-1.0, 1.0]` (typically positive — a strongly
/// tonal track scores 0.7-0.9, atonal noise drops below 0.2).
pub fn estimate_key_with_confidence(
    samples_mono: &[f32],
    sample_rate: u32,
) -> Result<(String, f32)> {
    if sample_rate == 0 {
        return Err(Error::Analysis("sample_rate is zero".into()));
    }
    if samples_mono.len() < FFT_SIZE {
        return Err(Error::Analysis(
            "audio shorter than chroma FFT window".into(),
        ));
    }
    let chroma = compute_chroma(samples_mono, sample_rate)?;

    let mut best_key = "C major".to_string();
    let mut best_corr = f32::NEG_INFINITY;
    for (root, name) in PITCH_NAMES.iter().enumerate() {
        let major = rotate(&KRUMHANSL_MAJOR, root);
        let minor = rotate(&KRUMHANSL_MINOR, root);
        let cor_maj = pearson(&chroma, &major);
        let cor_min = pearson(&chroma, &minor);
        if cor_maj > best_corr {
            best_corr = cor_maj;
            best_key = format!("{name} major");
        }
        if cor_min > best_corr {
            best_corr = cor_min;
            best_key = format!("{name} minor");
        }
    }
    Ok((best_key, best_corr))
}

/// Compute a 12-bin chroma vector (pitch-class energy distribution).
pub(crate) fn compute_chroma(samples_mono: &[f32], sample_rate: u32) -> Result<[f32; 12]> {
    let mut planner = RealFftPlanner::<f32>::new();
    let r2c = planner.plan_fft_forward(FFT_SIZE);
    let mut input = vec![0.0f32; FFT_SIZE];
    let mut output = vec![Complex::<f32>::new(0.0, 0.0); FFT_SIZE / 2 + 1];

    let window = hann_window(FFT_SIZE);
    let n_frames = if samples_mono.len() >= FFT_SIZE {
        (samples_mono.len() - FFT_SIZE) / HOP + 1
    } else {
        0
    };
    if n_frames == 0 {
        return Err(Error::Analysis(
            "audio too short for chroma extraction".into(),
        ));
    }

    let mut chroma = [0.0f32; 12];
    let bin_hz = sample_rate as f32 / FFT_SIZE as f32;

    // Frequency of MIDI 12 (C0) ≈ 16.3516 Hz.
    let c0 = 16.351_597f32;

    for f in 0..n_frames {
        let start = f * HOP;
        for i in 0..FFT_SIZE {
            input[i] = samples_mono[start + i] * window[i];
        }
        r2c.process(&mut input, &mut output)
            .map_err(|e| Error::Analysis(format!("FFT failed: {e}")))?;

        // Skip DC bin.
        for (i, c) in output.iter().enumerate().skip(1) {
            let freq = i as f32 * bin_hz;
            // Restrict to the musically relevant range (≈ A0..C8).
            if !(27.5..=4_186.0).contains(&freq) {
                continue;
            }
            let mag = (c.re * c.re + c.im * c.im).sqrt();
            // Pitch class index 0..12 (0 = C).
            let pc = ((12.0 * (freq / c0).log2()).round() as i32).rem_euclid(12) as usize;
            chroma[pc] += mag;
        }
    }

    // Normalise to unit sum so Pearson correlation is scale-invariant.
    let sum: f32 = chroma.iter().sum();
    if sum > 0.0 {
        for v in chroma.iter_mut() {
            *v /= sum;
        }
    }
    Ok(chroma)
}

fn hann_window(n: usize) -> Vec<f32> {
    (0..n)
        .map(|i| {
            let x = std::f32::consts::PI * 2.0 * i as f32 / (n - 1) as f32;
            0.5 - 0.5 * x.cos()
        })
        .collect()
}

/// Rotate a 12-element array left by `n` (so a profile keyed for C
/// becomes one keyed for the requested root).
fn rotate(a: &[f32; 12], n: usize) -> [f32; 12] {
    let mut out = [0.0f32; 12];
    for i in 0..12 {
        // Profile written for C is at index 0; to align with root `n`
        // we want the root note to land at index `n` in the chroma, so
        // out[(i + n) % 12] = a[i].
        out[(i + n) % 12] = a[i];
    }
    out
}

/// Pearson correlation between two equal-length vectors.
pub(crate) fn pearson(a: &[f32; 12], b: &[f32; 12]) -> f32 {
    let mean_a = a.iter().sum::<f32>() / a.len() as f32;
    let mean_b = b.iter().sum::<f32>() / b.len() as f32;
    let mut num = 0.0f32;
    let mut da = 0.0f32;
    let mut db = 0.0f32;
    for i in 0..a.len() {
        let xa = a[i] - mean_a;
        let xb = b[i] - mean_b;
        num += xa * xb;
        da += xa * xa;
        db += xb * xb;
    }
    let denom = (da * db).sqrt();
    if denom == 0.0 {
        0.0
    } else {
        num / denom
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Generate a sum-of-sines signal at the given pitch frequencies.
    fn chord(sample_rate: u32, duration_s: f32, freqs: &[f32]) -> Vec<f32> {
        let n = (duration_s * sample_rate as f32) as usize;
        let mut out = vec![0.0f32; n];
        for &f in freqs {
            for (i, s) in out.iter_mut().enumerate() {
                let t = i as f32 / sample_rate as f32;
                *s += (2.0 * std::f32::consts::PI * f * t).sin() / freqs.len() as f32;
            }
        }
        out
    }

    #[test]
    fn rotate_shifts_root() {
        // Major profile keyed to C: index 0 has the largest mass (6.35).
        // After rotate by 1, that mass should be at index 1 (C# major).
        let r = rotate(&KRUMHANSL_MAJOR, 1);
        assert!((r[1] - KRUMHANSL_MAJOR[0]).abs() < 1e-6);
    }

    #[test]
    fn pearson_perfect_match() {
        let a = KRUMHANSL_MAJOR;
        let p = pearson(&a, &a);
        assert!((p - 1.0).abs() < 1e-5);
    }

    #[test]
    fn chroma_pure_c_concentrates_at_index_0() {
        // 261.63 Hz = middle C.
        let sr = 22_050;
        let signal = chord(sr, 2.0, &[261.63]);
        let chroma = compute_chroma(&signal, sr).expect("chroma ok");
        let max = chroma
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
            .unwrap()
            .0;
        assert_eq!(max, 0, "expected pitch-class C (0), got chroma={chroma:?}");
    }

    #[test]
    fn detects_c_major_chord_progression() {
        // Three triads: C major (C-E-G), F major (F-A-C), G major (G-B-D).
        // All three live within the C-major key signature.
        let sr = 22_050;
        let mut sig = Vec::new();
        // C major
        sig.extend(chord(sr, 1.0, &[261.63, 329.63, 392.00]));
        // F major
        sig.extend(chord(sr, 1.0, &[349.23, 440.00, 523.25]));
        // G major
        sig.extend(chord(sr, 1.0, &[392.00, 493.88, 587.33]));
        // Back to C
        sig.extend(chord(sr, 1.0, &[261.63, 329.63, 392.00]));
        let key = estimate_key(&sig, sr).expect("ok");
        assert_eq!(key, "C major", "expected C major, got {key}");
    }

    #[test]
    fn detects_a_minor_chord_progression() {
        // A minor: A-C-E, D minor: D-F-A, E major: E-G#-B (V in A minor).
        let sr = 22_050;
        let mut sig = Vec::new();
        // A minor (twice — emphasise tonic)
        sig.extend(chord(sr, 1.0, &[220.00, 261.63, 329.63]));
        sig.extend(chord(sr, 1.0, &[220.00, 261.63, 329.63]));
        // D minor
        sig.extend(chord(sr, 1.0, &[293.66, 349.23, 440.00]));
        // E major (V) — note G# pulls toward A minor over A major
        sig.extend(chord(sr, 1.0, &[329.63, 415.30, 493.88]));
        // Resolve to A minor
        sig.extend(chord(sr, 1.0, &[220.00, 261.63, 329.63]));
        let key = estimate_key(&sig, sr).expect("ok");
        assert_eq!(key, "A minor", "expected A minor, got {key}");
    }

    #[test]
    fn confidence_bounded() {
        let sr = 22_050;
        let sig = chord(sr, 2.0, &[261.63, 329.63, 392.00]);
        let (_key, conf) = estimate_key_with_confidence(&sig, sr).expect("ok");
        assert!(
            (-1.0..=1.0).contains(&conf),
            "confidence {conf} out of range"
        );
    }
}
