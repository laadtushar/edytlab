//! Key detection: peak-picked chroma, the bass counted twice, correlated
//! with 24 key profiles.
//!
//! ## What went wrong before (#696)
//!
//! A sustained B-flat minor triad read as F major. Two causes:
//!
//! * **Chroma.** It summed the magnitude of every FFT bin into the nearest
//!   semitone, with a 4096-point window. At 233 Hz one bin is 10.8 Hz and a
//!   semitone is 13.8 Hz, so a note smeared over its neighbours, and the
//!   skirts of every note, summed over hundreds of bins, outweighed the notes.
//!   Now only spectral **peaks** count, each placed by parabolic
//!   interpolation of its log magnitude, in a window long enough (about
//!   0.37 s) to separate semitones down to 100 Hz, and the tuning of the
//!   recording is estimated and removed.
//! * **Profile.** Krumhansl & Kessler (1982) ratings are, in the words of the
//!   Humdrum `keycor` manual (Craig Sapp, <https://extras.humdrum.org/man/keycor/>),
//!   a "strong tendency to identify the dominant key as the tonic": F is the
//!   dominant of B-flat. On the corpus below it gets a tonic-and-dominant
//!   vamp (I-V-I-V) right in 0 of 24 keys. The other sets in
//!   that manual were measured too, and
//!   [`ProfileSet::TemperleyKostkaPayne`] (D. Temperley's weightings from the
//!   Kostka–Payne textbook corpus, *Music and Probability*, MIT Press 2007;
//!   values as in music21's `analysis.discrete.TemperleyKostkaPayne`) is the
//!   one that gets every key right on every case of the corpus but the
//!   ambiguous loop, and reads the pads of the three DJ demo tracks (A, G and
//!   B major) as they are. The manual says it has a "strong tendency to identify
//!   the relative major as the tonic in minor keys", the confusion #696 also
//!   names; on this corpus it made none of those misses (the 4-chord loop
//!   that is the same chords in either reading is the exception, for every
//!   set), and the runner-up in the result is there so that a close call
//!   shows. Bellman–Budge ("no particular tendencies") read the demos as
//!   their subdominant keys. `tests/key_corpus.rs` prints the table.
//!
//! ## Tonic against dominant, minor against relative major
//!
//! Profiles alone leave a tie between a key and its neighbours that share its
//! notes. It is broken by the **bass**: a second chroma of the 40–250 Hz peaks
//! is added to the first (weight [`BASS_WEIGHT`]), the usual way to say that
//! the lowest note is the likeliest root. On the corpus that takes the
//! tonic-and-dominant vamp from 0 of 24 to 24 of 24 under the chosen profile
//! set and costs nothing elsewhere; it does not change any relative-major
//! case there, because a bass that stays on the tonic is already in the full
//! chroma. It is one correlation in the end, so the scores of the winner and
//! the runner-up are comparable Pearson coefficients.
//!
//! Profile constants are the published values; see each constant.

use realfft::num_complex::Complex;
use realfft::RealFftPlanner;

use crate::{Error, Result};

/// Window length in seconds for the chroma FFT (rounded to a power of two).
const WINDOW_SECONDS: f32 = 0.37;
/// Shortest window, samples. Shorter audio is an error.
const MIN_FFT: usize = 4096;
/// Lowest and highest frequency that counts toward the chroma, Hz.
const CHROMA_RANGE_HZ: (f32, f32) = (50.0, 5_000.0);
/// The bass band, Hz.
const BASS_RANGE_HZ: (f32, f32) = (40.0, 250.0);
/// A peak counts if it is at least this fraction of the frame's strongest.
const PEAK_FLOOR: f32 = 0.02;
/// Peak weight is `magnitude.powf(PEAK_COMPRESSION)`: a loud note counts more
/// than a quiet one, but not by its full amplitude.
const PEAK_COMPRESSION: f32 = 0.5;
/// How much the bass chroma counts next to the full chroma, which has unit
/// sum. Chosen on the synthetic corpus (`tests/key_corpus.rs`): 0.5 is the
/// only weight at which the chosen profile set gets every case right.
pub const BASS_WEIGHT: f32 = 0.5;

/// Krumhansl–Kessler 1982 major key profile (C major). Krumhansl, *Cognitive
/// Foundations of Musical Pitch*, 1990, p. 30.
pub const KRUMHANSL_MAJOR: [f32; 12] = [
    6.35, 2.23, 3.48, 2.33, 4.38, 4.09, 2.52, 5.19, 2.39, 3.66, 2.29, 2.88,
];
/// Krumhansl–Kessler 1982 minor key profile (C minor).
pub const KRUMHANSL_MINOR: [f32; 12] = [
    6.33, 2.68, 3.52, 5.38, 2.60, 3.53, 2.54, 4.75, 3.98, 2.69, 3.34, 3.17,
];

/// Bellman–Budge major profile (C major), as in Humdrum `keycor`
/// (<https://extras.humdrum.org/man/keycor/>) and music21
/// `analysis.discrete.BellmanBudge`.
pub const BELLMAN_BUDGE_MAJOR: [f32; 12] = [
    16.80, 0.86, 12.95, 1.41, 13.49, 11.93, 1.25, 20.28, 1.80, 8.04, 0.62, 10.57,
];
/// Bellman–Budge minor profile (C minor).
pub const BELLMAN_BUDGE_MINOR: [f32; 12] = [
    18.16, 0.69, 12.99, 13.34, 1.07, 11.15, 1.38, 21.07, 7.49, 1.53, 0.92, 10.21,
];

/// Temperley (Kostka–Payne corpus) major profile (C major), as in Humdrum
/// `keycor` and music21 `analysis.discrete.TemperleyKostkaPayne`.
pub const TEMPERLEY_MAJOR: [f32; 12] = [
    0.748, 0.060, 0.488, 0.082, 0.670, 0.460, 0.096, 0.715, 0.104, 0.366, 0.057, 0.400,
];
/// Temperley (Kostka–Payne corpus) minor profile (C minor).
pub const TEMPERLEY_MINOR: [f32; 12] = [
    0.712, 0.084, 0.474, 0.618, 0.049, 0.460, 0.105, 0.747, 0.404, 0.067, 0.133, 0.330,
];

/// Aarden–Essen major profile (C major), as in Humdrum `keycor` and music21
/// `analysis.discrete.AardenEssen`.
pub const AARDEN_MAJOR: [f32; 12] = [
    17.7661, 0.145624, 14.9265, 0.160186, 19.8049, 11.3587, 0.291248, 22.062, 0.145624, 8.15494,
    0.232998, 4.95122,
];
/// Aarden–Essen minor profile (C minor). music21 notes the source of the
/// minor values is uncertain.
pub const AARDEN_MINOR: [f32; 12] = [
    18.2648, 0.737619, 14.0499, 16.8599, 0.702494, 14.4362, 0.702494, 18.6161, 4.56621, 1.93186,
    7.37619, 1.75623,
];

/// A set of key profiles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileSet {
    KrumhanslKessler,
    BellmanBudge,
    TemperleyKostkaPayne,
    AardenEssen,
}

impl ProfileSet {
    /// The set [`estimate_key`] uses.
    pub const DEFAULT: ProfileSet = ProfileSet::TemperleyKostkaPayne;

    pub const ALL: [ProfileSet; 4] = [
        ProfileSet::KrumhanslKessler,
        ProfileSet::BellmanBudge,
        ProfileSet::TemperleyKostkaPayne,
        ProfileSet::AardenEssen,
    ];

    pub fn name(self) -> &'static str {
        match self {
            ProfileSet::KrumhanslKessler => "Krumhansl-Kessler",
            ProfileSet::BellmanBudge => "Bellman-Budge",
            ProfileSet::TemperleyKostkaPayne => "Temperley-Kostka-Payne",
            ProfileSet::AardenEssen => "Aarden-Essen",
        }
    }

    fn major(self) -> &'static [f32; 12] {
        match self {
            ProfileSet::KrumhanslKessler => &KRUMHANSL_MAJOR,
            ProfileSet::BellmanBudge => &BELLMAN_BUDGE_MAJOR,
            ProfileSet::TemperleyKostkaPayne => &TEMPERLEY_MAJOR,
            ProfileSet::AardenEssen => &AARDEN_MAJOR,
        }
    }

    fn minor(self) -> &'static [f32; 12] {
        match self {
            ProfileSet::KrumhanslKessler => &KRUMHANSL_MINOR,
            ProfileSet::BellmanBudge => &BELLMAN_BUDGE_MINOR,
            ProfileSet::TemperleyKostkaPayne => &TEMPERLEY_MINOR,
            ProfileSet::AardenEssen => &AARDEN_MINOR,
        }
    }
}

const PITCH_NAMES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];

/// The key, and how close the next best was.
#[derive(Debug, Clone, PartialEq)]
pub struct KeyEstimate {
    /// `"<root> <mode>"`, e.g. `"A# minor"`.
    pub key: String,
    /// Pearson correlation of the chroma with the key's profile, `-1..=1`. A
    /// strongly tonal track scores 0.7–0.95.
    pub score: f32,
    /// The second-best key.
    pub runner_up: String,
    /// Its correlation. A runner-up within about 0.05 of `score` (a relative
    /// major, or the key a fifth away) means the call is a close one.
    pub runner_up_score: f32,
}

/// Estimate the key of a mono audio buffer. Returns `"<root> <mode>"`,
/// e.g. `"C major"`, `"F# minor"`.
pub fn estimate_key(samples_mono: &[f32], sample_rate: u32) -> Result<String> {
    Ok(estimate_key_detailed(samples_mono, sample_rate)?.key)
}

/// Like [`estimate_key`] but also returns a Pearson correlation as a
/// confidence value in `[-1.0, 1.0]` (typically positive — a strongly
/// tonal track scores 0.7-0.9, atonal noise drops below 0.2).
pub fn estimate_key_with_confidence(
    samples_mono: &[f32],
    sample_rate: u32,
) -> Result<(String, f32)> {
    let k = estimate_key_detailed(samples_mono, sample_rate)?;
    Ok((k.key, k.score))
}

/// The key, its score, and the runner-up with its score.
pub fn estimate_key_detailed(samples_mono: &[f32], sample_rate: u32) -> Result<KeyEstimate> {
    estimate_key_with(samples_mono, sample_rate, ProfileSet::DEFAULT, BASS_WEIGHT)
}

/// [`estimate_key_detailed`] with the profile set and the bass weight chosen,
/// so `tests/key_corpus.rs` can compare them.
pub fn estimate_key_with(
    samples_mono: &[f32],
    sample_rate: u32,
    profiles: ProfileSet,
    bass_weight: f32,
) -> Result<KeyEstimate> {
    if sample_rate == 0 {
        return Err(Error::Analysis("sample_rate is zero".into()));
    }
    if samples_mono.len() < MIN_FFT {
        return Err(Error::Analysis(
            "audio shorter than chroma FFT window".into(),
        ));
    }
    let c = chroma(samples_mono, sample_rate)?;
    estimate_key_from_chroma(&c, profiles, bass_weight)
}

/// The key from a chroma already computed, so a caller comparing several
/// profile sets or bass weights on one signal does the FFTs once.
pub fn estimate_key_from_chroma(
    c: &Chroma,
    profiles: ProfileSet,
    bass_weight: f32,
) -> Result<KeyEstimate> {
    if c.all.iter().all(|v| *v == 0.0) {
        return Err(Error::Analysis(
            "no pitched content to find a key in".into(),
        ));
    }
    let mut mix = c.all;
    for (m, b) in mix.iter_mut().zip(c.bass) {
        *m += bass_weight * b;
    }

    let mut scored: Vec<(f32, String)> = Vec::with_capacity(24);
    for (root, name) in PITCH_NAMES.iter().enumerate() {
        scored.push((
            pearson(&mix, &rotate(profiles.major(), root)),
            format!("{name} major"),
        ));
        scored.push((
            pearson(&mix, &rotate(profiles.minor(), root)),
            format!("{name} minor"),
        ));
    }
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    let (score, key) = scored[0].clone();
    let (runner_up_score, runner_up) = scored[1].clone();
    Ok(KeyEstimate {
        key,
        score,
        runner_up,
        runner_up_score,
    })
}

/// Pitch-class energy of the whole spectrum and of the bass, each summing
/// to 1 (or all zero for silence). Index 0 is C.
#[derive(Debug, Clone, PartialEq)]
pub struct Chroma {
    pub all: [f32; 12],
    pub bass: [f32; 12],
}

fn fft_size(sample_rate: u32, len: usize) -> usize {
    let target = (sample_rate as f32 * WINDOW_SECONDS).max(MIN_FFT as f32);
    let mut n = 1usize << (target.log2().round() as u32);
    // Never longer than the audio.
    while n > len && n > MIN_FFT {
        n /= 2;
    }
    n.max(MIN_FFT)
}

/// One spectral peak: where it is in semitones (MIDI, fractional), how
/// strong, and whether it is in the bass band.
struct Peak {
    midi: f32,
    weight: f32,
    bass: bool,
}

/// The chroma of a mono buffer. Errors if the audio is shorter than 4096
/// samples.
pub fn chroma(samples_mono: &[f32], sample_rate: u32) -> Result<Chroma> {
    if sample_rate == 0 {
        return Err(Error::Analysis("sample_rate is zero".into()));
    }
    let fft = fft_size(sample_rate, samples_mono.len());
    if samples_mono.len() < fft {
        return Err(Error::Analysis(
            "audio too short for chroma extraction".into(),
        ));
    }
    let hop = fft / 2;
    let n_frames = (samples_mono.len() - fft) / hop + 1;

    let mut planner = RealFftPlanner::<f32>::new();
    let r2c = planner.plan_fft_forward(fft);
    let mut input = vec![0.0f32; fft];
    let mut output = vec![Complex::<f32>::new(0.0, 0.0); fft / 2 + 1];
    let window = hann_window(fft);
    let bin_hz = sample_rate as f32 / fft as f32;
    let lo_bin = ((BASS_RANGE_HZ.0.min(CHROMA_RANGE_HZ.0) / bin_hz).floor() as usize).max(2);
    let hi_bin = ((CHROMA_RANGE_HZ.1 / bin_hz).ceil() as usize).min(fft / 2 - 1);

    let mut peaks: Vec<Peak> = Vec::new();
    let mut mags = vec![0.0f32; fft / 2 + 1];
    for f in 0..n_frames {
        let start = f * hop;
        for i in 0..fft {
            input[i] = samples_mono[start + i] * window[i];
        }
        r2c.process(&mut input, &mut output)
            .map_err(|e| Error::Analysis(format!("FFT failed: {e}")))?;
        for (m, c) in mags.iter_mut().zip(&output) {
            *m = (c.re * c.re + c.im * c.im).sqrt();
        }
        let frame_max = mags[lo_bin..=hi_bin].iter().copied().fold(0.0, f32::max);
        if frame_max <= 1e-9 {
            continue;
        }
        let floor = frame_max * PEAK_FLOOR;
        for k in lo_bin..=hi_bin {
            let (a, b, c) = (mags[k - 1], mags[k], mags[k + 1]);
            if b < floor || b <= a || b < c {
                continue;
            }
            // Parabolic interpolation on log magnitude: a Hann-windowed sine
            // is close to a parabola there, which puts the peak within a
            // few cents of the true frequency.
            let (la, lb, lc) = (a.max(1e-12).ln(), b.ln(), c.max(1e-12).ln());
            let denom = la - 2.0 * lb + lc;
            let delta = if denom.abs() > 1e-9 {
                (0.5 * (la - lc) / denom).clamp(-0.5, 0.5)
            } else {
                0.0
            };
            let freq = (k as f32 + delta) * bin_hz;
            let in_chroma = (CHROMA_RANGE_HZ.0..=CHROMA_RANGE_HZ.1).contains(&freq);
            let in_bass = (BASS_RANGE_HZ.0..=BASS_RANGE_HZ.1).contains(&freq);
            if !in_chroma && !in_bass {
                continue;
            }
            peaks.push(Peak {
                midi: 69.0 + 12.0 * (freq / 440.0).log2(),
                weight: b.powf(PEAK_COMPRESSION),
                bass: in_bass,
            });
        }
    }

    // Tuning: the circular mean of each peak's distance from the nearest
    // semitone, weighted by strength. A recording tuned to 432 Hz or a tape
    // that ran slow would otherwise put every note between two classes.
    let (mut sx, mut sy) = (0.0f32, 0.0f32);
    for p in &peaks {
        let frac = p.midi - p.midi.round();
        let ang = std::f32::consts::TAU * frac;
        sx += p.weight * ang.cos();
        sy += p.weight * ang.sin();
    }
    let tuning = if sx == 0.0 && sy == 0.0 {
        0.0
    } else {
        sy.atan2(sx) / std::f32::consts::TAU
    };

    let mut all = [0.0f32; 12];
    let mut bass = [0.0f32; 12];
    for p in &peaks {
        let pc = ((p.midi - tuning).round() as i32).rem_euclid(12) as usize;
        let freq = 440.0 * 2f32.powf((p.midi - 69.0) / 12.0);
        if (CHROMA_RANGE_HZ.0..=CHROMA_RANGE_HZ.1).contains(&freq) {
            all[pc] += p.weight;
        }
        if p.bass {
            bass[pc] += p.weight;
        }
    }
    normalise(&mut all);
    normalise(&mut bass);
    Ok(Chroma { all, bass })
}

fn normalise(v: &mut [f32; 12]) {
    let sum: f32 = v.iter().sum();
    if sum > 0.0 {
        for x in v.iter_mut() {
            *x /= sum;
        }
    }
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

    /// Compute a 12-bin chroma vector (pitch-class energy distribution).
    fn compute_chroma(samples_mono: &[f32], sample_rate: u32) -> Result<[f32; 12]> {
        Ok(chroma(samples_mono, sample_rate)?.all)
    }

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
    fn chroma_of_a_triad_is_three_classes_not_a_smear() {
        // B-flat minor: B-flat3, D-flat4, F4 at 44.1 kHz. The old chroma put
        // a quarter of its weight on F and spread the rest over the others.
        let sr = 44_100;
        let signal = chord(sr, 4.0, &[233.08, 277.18, 349.23]);
        let c = compute_chroma(&signal, sr).expect("chroma ok");
        let on_triad: f32 = [10usize, 1, 5].iter().map(|&i| c[i]).sum();
        assert!(on_triad > 0.95, "triad holds {on_triad:.3} of {c:?}");
    }

    #[test]
    fn chroma_follows_the_tuning_of_the_recording() {
        // A at 432 Hz is 31.8 cents flat; the notes are still A, C#, E.
        let sr = 44_100;
        let f = |semis: f32| 432.0 * 2f32.powf(semis / 12.0);
        let signal = chord(sr, 4.0, &[f(0.0), f(4.0), f(7.0)]);
        let c = compute_chroma(&signal, sr).expect("chroma ok");
        let on_triad: f32 = [9usize, 1, 4].iter().map(|&i| c[i]).sum();
        assert!(on_triad > 0.95, "triad holds {on_triad:.3} of {c:?}");
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

    #[test]
    fn the_runner_up_is_a_different_key_with_a_lower_score() {
        let sr = 22_050;
        let sig = chord(sr, 2.0, &[261.63, 329.63, 392.00]);
        let k = estimate_key_detailed(&sig, sr).expect("ok");
        assert_ne!(k.key, k.runner_up);
        assert!(k.runner_up_score <= k.score, "{k:?}");
    }

    #[test]
    fn silence_has_no_key_to_find() {
        assert!(estimate_key_detailed(&vec![0.0; 22_050], 22_050).is_err());
    }
}
