//! One decode test per importable format (#764).
//!
//! The fixtures in `tests/fixtures/` are a half second of synthetic tone
//! each (CC0, made with the commands in `tests/fixtures/README.md`), so
//! every test knows what it should hear: the frequency of each channel,
//! the rate, the channel count and, within a stated tolerance, the length.
//!
//! Tolerances:
//! - **Lossless (AIFF, FLAC):** the length is exact and each sample is
//!   within 1.5 LSB of the synthesised sine (16-bit) or 1e-4 (24-bit).
//! - **Lossy (Ogg Vorbis, MP3):** the length may be padded by a codec
//!   block at the ends (`LOSSY_LENGTH_SLACK_SECS`), the frequency is
//!   found to within `FREQ_TOLERANCE_HZ`, and the level to within
//!   `LOSSY_LEVEL_TOLERANCE` of the 0.5 amplitude.

use std::path::{Path, PathBuf};

use audio_decoder::{decode_file, probe_sample_rate, DecodedAudio};

/// A pure tone's estimated frequency must be this close to the encoded one.
const FREQ_TOLERANCE_HZ: f32 = 2.0;
/// A lossy file may be longer or shorter than what was encoded by this much
/// (priming and the padding of the last block).
const LOSSY_LENGTH_SLACK_SECS: f32 = 0.06;
/// The fixtures' sine has an amplitude of 0.5; a lossy codec may be off by this.
const LOSSY_LEVEL_TOLERANCE: f32 = 0.1;
const DURATION_SECS: f32 = 0.5;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

/// The samples of one channel.
fn channel(d: &DecodedAudio, c: usize) -> Vec<f32> {
    d.samples
        .iter()
        .skip(c)
        .step_by(d.channels as usize)
        .copied()
        .collect()
}

/// The frequency of the strongest component of `x`: a Hann-windowed FFT over
/// the middle of the signal (the ends hold codec padding), with the peak
/// refined by a parabola through the log magnitudes of its three bins.
fn peak_hz(x: &[f32], rate: u32) -> f32 {
    use rustfft::num_complex::Complex;
    let n = 16_384usize;
    assert!(x.len() >= n + 2_000, "too short to analyse: {}", x.len());
    let start = (x.len() - n) / 2;
    let mut buf: Vec<Complex<f32>> = x[start..start + n]
        .iter()
        .enumerate()
        .map(|(i, &s)| {
            let w = 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / n as f32).cos();
            Complex::new(s * w, 0.0)
        })
        .collect();
    rustfft::FftPlanner::<f32>::new()
        .plan_fft_forward(n)
        .process(&mut buf);
    let mag: Vec<f32> = buf[..n / 2].iter().map(|c| c.norm()).collect();
    let (k, _) = mag
        .iter()
        .enumerate()
        .skip(1)
        .fold((1, 0.0f32), |a, (i, &m)| if m > a.1 { (i, m) } else { a });
    let (a, b, c) = (
        mag[k - 1].max(1e-12).ln(),
        mag[k].max(1e-12).ln(),
        mag[k + 1].max(1e-12).ln(),
    );
    let delta = 0.5 * (a - c) / (a - 2.0 * b + c);
    (k as f32 + delta) * rate as f32 / n as f32
}

fn peak(x: &[f32]) -> f32 {
    x.iter().fold(0.0, |m, s| m.max(s.abs()))
}

/// What `tests/fixtures/README.md` encoded: one tone per channel.
struct Expect {
    rate: u32,
    tones_hz: &'static [f32],
}

fn assert_tones(d: &DecodedAudio, e: &Expect) {
    assert_eq!(d.sample_rate, e.rate, "sample rate");
    assert_eq!(d.channels as usize, e.tones_hz.len(), "channel count");
    for (c, &hz) in e.tones_hz.iter().enumerate() {
        let got = peak_hz(&channel(d, c), d.sample_rate);
        assert!(
            (got - hz).abs() <= FREQ_TOLERANCE_HZ,
            "channel {c}: found {got:.2} Hz, encoded {hz} Hz (tolerance {FREQ_TOLERANCE_HZ} Hz)"
        );
    }
}

fn assert_lossy_length_and_level(d: &DecodedAudio) {
    let secs = (d.samples.len() / d.channels as usize) as f32 / d.sample_rate as f32;
    assert!(
        (secs - DURATION_SECS).abs() <= LOSSY_LENGTH_SLACK_SECS,
        "length {secs:.4} s, encoded {DURATION_SECS} s (slack {LOSSY_LENGTH_SLACK_SECS} s)"
    );
    for c in 0..d.channels as usize {
        let p = peak(&channel(d, c));
        assert!(
            (p - 0.5).abs() <= LOSSY_LEVEL_TOLERANCE,
            "channel {c}: peak {p:.3}, encoded 0.5 (tolerance {LOSSY_LEVEL_TOLERANCE})"
        );
    }
}

/// The synthesised sine of the fixture at frame `i`.
fn sine(hz: f32, rate: u32, i: usize) -> f32 {
    0.5 * (2.0 * std::f64::consts::PI * hz as f64 * i as f64 / rate as f64).sin() as f32
}

fn assert_exact_length(d: &DecodedAudio) {
    let frames = d.samples.len() / d.channels as usize;
    assert_eq!(frames, (d.sample_rate as f32 * DURATION_SECS) as usize);
}

fn assert_samples_match(d: &DecodedAudio, tones: &[f32], tolerance: f32) {
    let ch = d.channels as usize;
    let mut worst = 0.0f32;
    for i in 0..d.samples.len() / ch {
        for (c, &hz) in tones.iter().enumerate() {
            worst = worst.max((d.samples[i * ch + c] - sine(hz, d.sample_rate, i)).abs());
        }
    }
    assert!(
        worst <= tolerance,
        "worst sample error {worst}, tolerance {tolerance}"
    );
}

// --- Ogg Vorbis -----------------------------------------------------------

#[test]
fn ogg_vorbis_mono_decodes_to_the_encoded_tone() {
    let path = fixture("tone_440_mono_44100.ogg");
    let d = decode_file(&path).expect("decode ogg");
    assert_tones(
        &d,
        &Expect {
            rate: 44_100,
            tones_hz: &[440.0],
        },
    );
    assert_lossy_length_and_level(&d);
    assert_eq!(probe_sample_rate(&path).unwrap(), 44_100);
}

#[test]
fn ogg_vorbis_stereo_keeps_each_channel_and_its_rate() {
    let path = fixture("tones_440_880_stereo_48000.ogg");
    let d = decode_file(&path).expect("decode ogg");
    assert_tones(
        &d,
        &Expect {
            rate: 48_000,
            tones_hz: &[440.0, 880.0],
        },
    );
    assert_lossy_length_and_level(&d);
    assert_eq!(probe_sample_rate(&path).unwrap(), 48_000);
}

// --- AIFF -----------------------------------------------------------------

#[test]
fn aiff_16_bit_mono_decodes_sample_for_sample() {
    let path = fixture("tone_440_mono_44100_s16.aiff");
    let d = decode_file(&path).expect("decode aiff");
    assert_tones(
        &d,
        &Expect {
            rate: 44_100,
            tones_hz: &[440.0],
        },
    );
    assert_exact_length(&d);
    // 16-bit: one LSB is 1/32768; allow 1.5 for the converter's rounding.
    assert_samples_match(&d, &[440.0], 1.5 / 32_768.0);
    assert_eq!(probe_sample_rate(&path).unwrap(), 44_100);
}

#[test]
fn aiff_24_bit_stereo_decodes_sample_for_sample_in_channel_order() {
    let path = fixture("tones_440_880_stereo_48000_s24.aiff");
    let d = decode_file(&path).expect("decode aiff");
    assert_tones(
        &d,
        &Expect {
            rate: 48_000,
            tones_hz: &[440.0, 880.0],
        },
    );
    assert_exact_length(&d);
    assert_samples_match(&d, &[440.0, 880.0], 1e-4);
    assert_eq!(probe_sample_rate(&path).unwrap(), 48_000);
}

// --- Formats that already decoded: kept honest next to the new ones --------

#[test]
fn flac_decodes_to_the_encoded_tone() {
    let d = decode_file(&fixture("tone_440_mono_44100.flac")).expect("decode flac");
    assert_tones(
        &d,
        &Expect {
            rate: 44_100,
            tones_hz: &[440.0],
        },
    );
    assert_exact_length(&d);
    assert_samples_match(&d, &[440.0], 1.5 / 32_768.0);
}
