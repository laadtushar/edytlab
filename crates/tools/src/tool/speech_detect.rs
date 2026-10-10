//! Where the speech is, found from the audio alone (#168).
//!
//! `duck_under_speech` is most accurate when it keys on a transcript,
//! but the transcript needs `transcribe`, and the Whisper decoder has
//! not shipped (#384). Most of what ducking needs is *where* the speech
//! is, not *what was said*, and the audio can answer that without a
//! model.
//!
//! ## Why level alone is not enough
//!
//! A sidechain compressor keys on level. A breath is loud enough to
//! cross any threshold that a quiet line also crosses, so a threshold
//! low enough to catch the quiet line fires on every breath, click and
//! patch of hiss as well.
//!
//! ## Why a voicing gate fixes it
//!
//! Speech that carries a line is pitched: the vocal folds make a
//! periodic waveform, and a periodic waveform correlates with a copy of
//! itself shifted by one pitch period. A breath, a click or hiss is
//! noise, which does not. So a stretch of audio only counts as speech
//! if it is both above the track's own noise floor *and* has voiced
//! sound in it. Fricatives and stop consonants next to voiced sound ride
//! along inside the same segment; a standalone `s` or breath does not
//! start one.
//!
//! ## What it cannot do
//!
//! It tells pitched sound from unpitched sound, not a voice from an
//! instrument. A voice track that carries music reads as speech
//! throughout, and so does any other pitched sound that switches on and
//! off, such as a whistle or a phone tone. (A steady hum does not: it
//! never rises above its own noise floor.) Name a track that holds the
//! voice and little else.
//!
//! ## Determinism
//!
//! Plain sequential arithmetic, no parallelism: the same audio gives the
//! same passages on every run.

use audio_dsp::{Biquad, BiquadCoeffs, Processor};

use crate::tool::ducking::Passage;

/// Level and voicing are measured once per hop. Fine enough that a
/// passage's edges land within a hop of the real onset, coarse enough
/// that the voicing search stays cheap.
const HOP_S: f64 = 0.010;
/// The voicing window. Three or four pitch periods at the low end of a
/// male voice: long enough for the autocorrelation to be stable, short
/// enough to follow a changing pitch.
const WINDOW_S: f64 = 0.030;
/// Below this the band-pass drops the rumble and handling noise that
/// sits under a voice.
const BAND_LOW_HZ: f32 = 100.0;
/// Above this there is little voiced energy, only sibilance and hiss,
/// which would pull the voicing measure toward noise.
const BAND_HIGH_HZ: f32 = 3_500.0;
/// The analysis rate. Speech pitch lives far below 4 kHz, so a higher
/// rate only costs time.
const TARGET_RATE_HZ: u32 = 8_000;
/// Never speech below this, however quiet the rest of the track is.
const FLOOR_DBFS: f32 = -60.0;
/// The track's own speech level is taken from the loud end of what is
/// above the floor, not its maximum, so one clipped click cannot set it.
const SPEECH_LEVEL_PERCENTILE: f64 = 0.95;
/// The noise floor is taken from the quiet end of every hop.
const NOISE_PERCENTILE: f64 = 0.10;
/// Hops more than this below the track's own speech level are not
/// speech. Wide enough that a quiet line still clears it.
const SPEECH_RANGE_DB: f32 = 30.0;
/// And a hop must clear the noise floor by this much, so room tone
/// that is loud in absolute terms is not mistaken for speech.
const NOISE_MARGIN_DB: f32 = 6.0;
/// Quieter gaps shorter than this stay inside one segment: the closure
/// before a `p` or `t`, a breath between two words.
const BRIDGE_S: f64 = 0.15;
/// A segment needs this much pitched sound to count, so a click or a
/// cough that happens to ring does not.
const MIN_VOICED_S: f64 = 0.08;
/// Peak of the normalised autocorrelation above which a hop is voiced.
/// White noise peaks near 0.2 over this many lags; a pitched hop is
/// above 0.8.
const VOICING_MIN: f32 = 0.5;
/// Lowest fundamental searched for.
const PITCH_MIN_HZ: f64 = 70.0;
/// Highest fundamental searched for.
const PITCH_MAX_HZ: f64 = 400.0;
/// Frames filtered per pass, so only the decimated signal is ever held
/// at full length.
const CHUNK_FRAMES: usize = 4096;

/// Find the stretches of speech in interleaved `samples`.
///
/// Returns passages in time order, in seconds from the first frame.
/// Gaps between them are not joined here; the caller decides how short
/// a gap has to be before it stays ducked.
pub(crate) fn detect_speech(samples: &[f32], sample_rate: u32, channels: usize) -> Vec<Passage> {
    let channels = channels.max(1);
    if samples.is_empty() || sample_rate == 0 {
        return Vec::new();
    }

    let (x, rate) = band_limited_decimated(samples, sample_rate, channels);

    let hop = (rate * HOP_S).round().max(1.0) as usize;
    let win = (rate * WINDOW_S).round() as usize;
    let lag_min = (rate / PITCH_MAX_HZ).floor().max(1.0) as usize;
    let lag_max = (rate / PITCH_MIN_HZ).ceil() as usize;

    let hops = x.len().div_ceil(hop);
    let level_db: Vec<f32> = (0..hops)
        .map(|i| {
            let chunk = &x[i * hop..((i + 1) * hop).min(x.len())];
            let mean_sq = chunk.iter().map(|v| v * v).sum::<f32>() / chunk.len() as f32;
            10.0 * (mean_sq + 1e-12).log10()
        })
        .collect();

    let audible: Vec<f32> = level_db
        .iter()
        .copied()
        .filter(|&l| l >= FLOOR_DBFS)
        .collect();
    if audible.is_empty() {
        return Vec::new();
    }
    let speech_level = percentile(&audible, SPEECH_LEVEL_PERCENTILE);
    let noise_level = percentile(&level_db, NOISE_PERCENTILE);
    let threshold = (speech_level - SPEECH_RANGE_DB)
        .max(noise_level + NOISE_MARGIN_DB)
        .max(FLOOR_DBFS);
    let loud: Vec<bool> = level_db.iter().map(|&l| l >= threshold).collect();

    let bridge_hops = (BRIDGE_S / HOP_S).round() as usize;
    let mut need_voiced = 1usize;
    while ((need_voiced * hop) as f64 / rate) < MIN_VOICED_S {
        need_voiced += 1;
    }

    let mut out = Vec::new();
    for (first, last) in segments(&loud, bridge_hops) {
        // Stop counting at the quota: whether a segment clears it is
        // all that is asked, and a minute of speech should not cost a
        // minute of autocorrelation.
        let mut voiced = 0usize;
        for i in (first..=last).filter(|&i| loud[i]) {
            let p = i * hop;
            if p + win + lag_max > x.len() {
                continue;
            }
            if voicing(&x, p, win, lag_min, lag_max) >= VOICING_MIN {
                voiced += 1;
                if voiced >= need_voiced {
                    break;
                }
            }
        }
        if voiced < need_voiced {
            continue;
        }
        out.push(Passage {
            start_s: ((first * hop) as f64 / rate) as f32,
            end_s: (((last + 1) * hop).min(x.len()) as f64 / rate) as f32,
        });
    }
    out
}

/// Mono, band-passed to the speech band, and decimated to about
/// [`TARGET_RATE_HZ`]. Returns the signal and its rate.
///
/// Filtered at the source rate, then every `k`-th frame kept; the low
/// pass is what stops the dropped band folding down into the voicing
/// measure. Two sections of it, because one second-order section
/// aliases too much. The decimation phase follows the global frame
/// count, so the result does not depend on where the chunks fall.
fn band_limited_decimated(samples: &[f32], sample_rate: u32, channels: usize) -> (Vec<f32>, f64) {
    let k = ((sample_rate / TARGET_RATE_HZ) as usize).max(1);
    let rate = sample_rate as f64 / k as f64;

    let mut high = Biquad::new(BiquadCoeffs::high_pass(BAND_LOW_HZ, sample_rate), 1);
    let mut low_a = Biquad::new(BiquadCoeffs::low_pass(BAND_HIGH_HZ, sample_rate), 1);
    let mut low_b = Biquad::new(BiquadCoeffs::low_pass(BAND_HIGH_HZ, sample_rate), 1);

    let frames = samples.len() / channels;
    let mut x = Vec::with_capacity(frames / k + 1);
    let mut mono = Vec::with_capacity(CHUNK_FRAMES);
    let mut frame = 0usize;
    for chunk in samples[..frames * channels].chunks(CHUNK_FRAMES * channels) {
        mono.clear();
        mono.extend(
            chunk
                .chunks_exact(channels)
                .map(|f| f.iter().sum::<f32>() / channels as f32),
        );
        high.process(&mut mono, 1);
        low_a.process(&mut mono, 1);
        low_b.process(&mut mono, 1);
        for &v in &mono {
            if frame % k == 0 {
                x.push(v);
            }
            frame += 1;
        }
    }
    (x, rate)
}

/// The `p`-th percentile (0..=1) of `values`, nearest rank.
fn percentile(values: &[f32], p: f64) -> f32 {
    let mut sorted = values.to_vec();
    sorted.sort_by(f32::total_cmp);
    sorted[(((sorted.len() - 1) as f64) * p).round() as usize]
}

/// Maximal runs of loud hops as inclusive `(first, last)` hop indices,
/// with runs separated by fewer than `bridge_hops` quiet hops joined.
fn segments(loud: &[bool], bridge_hops: usize) -> Vec<(usize, usize)> {
    let mut runs: Vec<(usize, usize)> = Vec::new();
    let mut i = 0;
    while i < loud.len() {
        if !loud[i] {
            i += 1;
            continue;
        }
        let first = i;
        while i < loud.len() && loud[i] {
            i += 1;
        }
        let last = i - 1;
        match runs.last_mut() {
            Some(prev) if first - prev.1 - 1 < bridge_hops => prev.1 = last,
            _ => runs.push((first, last)),
        }
    }
    runs
}

/// How periodic `x[p..p + win]` is: the peak, over the lags a human
/// voice can have, of its normalised correlation with itself shifted by
/// that lag. Near 1 for a pitched sound, near 0 for noise.
fn voicing(x: &[f32], p: usize, win: usize, lag_min: usize, lag_max: usize) -> f32 {
    let a = &x[p..p + win];
    let energy_a: f32 = a.iter().map(|v| v * v).sum();
    let mut best = 0.0f32;
    for tau in lag_min..=lag_max {
        let b = &x[p + tau..p + tau + win];
        let mut dot = 0.0f32;
        let mut energy_b = 0.0f32;
        for (u, v) in a.iter().zip(b) {
            dot += u * v;
            energy_b += v * v;
        }
        let denom = (energy_a * energy_b).sqrt();
        if denom > 0.0 {
            best = best.max(dot / denom);
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    const TAU: f64 = std::f64::consts::TAU;

    /// Voiced sound: six harmonics of 140 Hz, falling as 1/k. Pitched,
    /// and about -21 dBFS at `amp` 0.1.
    fn voiced(sr: u32, secs: f64, amp: f32) -> Vec<f32> {
        voiced_at(sr, secs, amp, 140.0)
    }

    /// [`voiced`] at another fundamental.
    fn voiced_at(sr: u32, secs: f64, amp: f32, f0: f64) -> Vec<f32> {
        (0..(sr as f64 * secs) as usize)
            .map(|n| {
                let t = n as f64 / sr as f64;
                (1..=6)
                    .map(|k| amp as f64 / k as f64 * (TAU * k as f64 * f0 * t).sin())
                    .sum::<f64>() as f32
            })
            .collect()
    }

    /// White noise at a given RMS, from a fixed-seed xorshift so the
    /// tests do not need a random-number dependency.
    fn noise(sr: u32, secs: f64, rms: f32, seed: u32) -> Vec<f32> {
        let mut state = seed.max(1);
        // Uniform on [-1, 1) has an RMS of 1/sqrt(3).
        let scale = rms * 3f32.sqrt();
        (0..(sr as f64 * secs) as usize)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                (state as f32 / u32::MAX as f32 * 2.0 - 1.0) * scale
            })
            .collect()
    }

    fn rms(v: &[f32]) -> f32 {
        (v.iter().map(|s| s * s).sum::<f32>() / v.len() as f32).sqrt()
    }

    /// Add `signal` into `buf` starting at `at_s`.
    fn place(buf: &mut [f32], sr: u32, at_s: f64, signal: &[f32]) {
        let start = (at_s * sr as f64).round() as usize;
        for (d, s) in buf[start..].iter_mut().zip(signal) {
            *d += *s;
        }
    }

    fn silence(sr: u32, secs: f64) -> Vec<f32> {
        vec![0.0; (sr as f64 * secs) as usize]
    }

    fn assert_passage(p: &Passage, start: f64, end: f64, tol: f64) {
        assert!(
            (p.start_s as f64 - start).abs() <= tol && (p.end_s as f64 - end).abs() <= tol,
            "expected {start}..{end} (+-{tol}), got {}..{}",
            p.start_s,
            p.end_s
        );
    }

    #[test]
    fn finds_a_voiced_burst_at_its_place() {
        let sr = 48_000;
        let mut buf = silence(sr, 5.0);
        place(&mut buf, sr, 1.0, &voiced(sr, 1.0, 0.1));
        let found = detect_speech(&buf, sr, 1);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_passage(&found[0], 1.0, 2.0, 0.02);
    }

    /// The test that fails with level alone: a breath as loud as the
    /// voice is not speech.
    #[test]
    fn a_breath_is_not_speech() {
        let sr = 48_000;
        let line = voiced(sr, 1.0, 0.1);
        let breath = noise(sr, 0.5, rms(&line), 7);
        let mut buf = silence(sr, 5.0);
        place(&mut buf, sr, 1.0, &breath);
        place(&mut buf, sr, 3.0, &line);
        let found = detect_speech(&buf, sr, 1);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_passage(&found[0], 3.0, 4.0, 0.02);

        let hiss = noise(sr, 5.0, 0.05, 11);
        assert!(detect_speech(&hiss, sr, 1).is_empty(), "noise alone");
    }

    #[test]
    fn clicks_and_blips_are_not_speech() {
        let sr = 48_000;
        let mut clicks = silence(sr, 4.0);
        for at in [0.5, 1.2, 1.25, 2.7] {
            clicks[(at * sr as f64) as usize] = 0.9;
        }
        assert!(detect_speech(&clicks, sr, 1).is_empty(), "clicks");

        let mut blip = silence(sr, 4.0);
        place(&mut blip, sr, 1.0, &voiced(sr, 0.04, 0.1));
        assert!(detect_speech(&blip, sr, 1).is_empty(), "a 40 ms blip");
    }

    #[test]
    fn room_tone_is_not_speech() {
        let sr = 48_000;
        let mut buf = noise(sr, 10.0, 10f32.powf(-50.0 / 20.0), 3);
        place(&mut buf, sr, 2.0, &voiced(sr, 1.0, 0.1));
        place(&mut buf, sr, 6.0, &voiced(sr, 1.0, 0.1));
        let found = detect_speech(&buf, sr, 1);
        assert_eq!(found.len(), 2, "{found:?}");
        assert_passage(&found[0], 2.0, 3.0, 0.03);
        assert_passage(&found[1], 6.0, 7.0, 0.03);
    }

    /// Eighteen dB down is still a line.
    #[test]
    fn a_quiet_line_is_still_found() {
        let sr = 48_000;
        let mut buf = silence(sr, 8.0);
        place(&mut buf, sr, 1.0, &voiced(sr, 1.0, 0.1));
        place(
            &mut buf,
            sr,
            5.0,
            &voiced(sr, 1.0, 0.1 * 10f32.powf(-18.0 / 20.0)),
        );
        let found = detect_speech(&buf, sr, 1);
        assert_eq!(found.len(), 2, "{found:?}");
        assert_passage(&found[1], 5.0, 6.0, 0.03);
    }

    /// Catches a decimation or time-conversion slip: the same layout at
    /// three rates must land in the same place.
    #[test]
    fn same_answer_at_any_rate() {
        let mut first = Vec::new();
        for sr in [16_000u32, 44_100, 48_000] {
            let mut buf = silence(sr, 6.0);
            place(&mut buf, sr, 1.0, &voiced(sr, 1.0, 0.1));
            place(&mut buf, sr, 4.0, &voiced(sr, 1.5, 0.1));
            let found = detect_speech(&buf, sr, 1);
            assert_eq!(found.len(), 2, "{sr} Hz: {found:?}");
            assert_passage(&found[0], 1.0, 2.0, 0.03);
            assert_passage(&found[1], 4.0, 5.5, 0.03);
            first.push((found[0].start_s, found[0].end_s));
        }
        for pair in first.windows(2) {
            assert!((pair[0].0 - pair[1].0).abs() <= 0.02, "{first:?}");
            assert!((pair[0].1 - pair[1].1).abs() <= 0.02, "{first:?}");
        }
    }

    #[test]
    fn stereo_reads_like_mono() {
        let sr = 48_000;
        let line = voiced(sr, 1.0, 0.1);
        let mut mono = silence(sr, 4.0);
        place(&mut mono, sr, 1.0, &line);
        let want = detect_speech(&mono, sr, 1);
        assert_eq!(want.len(), 1);

        let both: Vec<f32> = mono.iter().flat_map(|&v| [v, v]).collect();
        assert_eq!(detect_speech(&both, sr, 2), want, "both channels");

        let left_only: Vec<f32> = mono.iter().flat_map(|&v| [v, 0.0]).collect();
        let found = detect_speech(&left_only, sr, 2);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_passage(&found[0], 1.0, 2.0, 0.02);
    }

    #[test]
    fn silence_and_empty_input() {
        assert!(detect_speech(&silence(48_000, 3.0), 48_000, 1).is_empty());
        assert!(detect_speech(&[], 48_000, 1).is_empty());
        assert!(detect_speech(&[0.5; 100], 0, 1).is_empty());
        assert!(detect_speech(&silence(48_000, 1.0), 48_000, 0).is_empty());
    }

    /// A stop consonant is a short gap inside a word, not the end of it.
    #[test]
    fn a_gap_inside_a_word_does_not_split_it() {
        let sr = 48_000;
        let mut buf = silence(sr, 4.0);
        place(&mut buf, sr, 1.0, &voiced(sr, 0.4, 0.1));
        place(&mut buf, sr, 1.5, &voiced(sr, 0.5, 0.1));
        let found = detect_speech(&buf, sr, 1);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_passage(&found[0], 1.0, 2.0, 0.02);
    }

    /// A steady hum is pitched, but it never rises above its own noise
    /// floor, so it is neither speech on its own nor a stretch of speech
    /// when it sits under some.
    #[test]
    fn a_steady_hum_is_not_speech() {
        let sr = 48_000;
        let hum: Vec<f32> = (0..sr as usize * 6)
            .map(|n| 0.02 * (TAU * 120.0 * n as f64 / sr as f64).sin() as f32)
            .collect();
        assert!(detect_speech(&hum, sr, 1).is_empty(), "hum alone");

        let mut buf = hum;
        place(&mut buf, sr, 2.0, &voiced(sr, 1.0, 0.1));
        let found = detect_speech(&buf, sr, 1);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_passage(&found[0], 2.0, 3.0, 0.03);
    }

    /// The pitch search has to cover a deep male voice and a high
    /// female one, not just the 140 Hz the other tests use.
    #[test]
    fn low_and_high_voices_are_both_found() {
        let sr = 48_000;
        for f0 in [85.0, 110.0, 220.0, 330.0] {
            let mut buf = silence(sr, 4.0);
            place(&mut buf, sr, 1.0, &voiced_at(sr, 1.0, 0.1, f0));
            let found = detect_speech(&buf, sr, 1);
            assert_eq!(found.len(), 1, "{f0} Hz: {found:?}");
            assert_passage(&found[0], 1.0, 2.0, 0.03);
        }
    }

    /// Two lines further apart than the bridge stay two.
    #[test]
    fn a_real_gap_does_split() {
        let sr = 48_000;
        let mut buf = silence(sr, 4.0);
        place(&mut buf, sr, 1.0, &voiced(sr, 0.5, 0.1));
        place(&mut buf, sr, 1.8, &voiced(sr, 0.5, 0.1));
        assert_eq!(detect_speech(&buf, sr, 1).len(), 2);
    }
}
