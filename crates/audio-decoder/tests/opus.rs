//! Opus import (#764): `.opus` files and Ogg files that hold Opus.
//!
//! The fixtures are in `tests/fixtures/`, made by the commands in its README.
//! Lengths are the ones `ffmpeg -i x.opus -f f32le -` writes and `ffprobe`
//! reports less the 312 frames of pre-skip it counts in `duration_ts`
//! (RFC 7845 §4: a stream holds `last granule - pre_skip` frames).

use std::path::{Path, PathBuf};

use audio_decoder::{decode_bytes, decode_file, probe_sample_rate, DecodeError, DecodedAudio};

const MONO: &str = "tone_440_mono_48000.opus";
const STEREO: &str = "tones_440_880_stereo_48000.opus";

/// ffprobe: `duration_ts=24312` with 312 frames of pre-skip; ffmpeg writes
/// 96,000 bytes of f32 mono, which is 24,000 frames (0.5 s exactly).
const MONO_FRAMES: usize = 24_000;
/// ffprobe: `duration_ts=46872`; ffmpeg writes 372,480 bytes of f32 stereo,
/// 46,560 frames: 0.97 s, which is not a whole number of 20 ms packets, so the
/// last packet is cut by the end trim.
const STEREO_FRAMES: usize = 46_560;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

fn channel(d: &DecodedAudio, c: usize) -> Vec<f32> {
    d.samples
        .iter()
        .skip(c)
        .step_by(d.channels as usize)
        .copied()
        .collect()
}

fn frames(d: &DecodedAudio) -> usize {
    d.samples.len() / d.channels as usize
}

/// The frequency of the strongest component of `x`: a Hann-windowed FFT over
/// the middle of the signal, the peak refined by a parabola through the log
/// magnitudes of its three bins.
fn peak_hz(x: &[f32], rate: u32) -> f32 {
    use rustfft::num_complex::Complex;
    let n = 16_384usize;
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

fn sine(hz: f32, i: usize) -> f32 {
    0.5 * (2.0 * std::f64::consts::PI * hz as f64 * i as f64 / 48_000.0).sin() as f32
}

/// RMS difference between channel `c` and the synthesised sine, over the
/// middle of the file (the ends hold the codec's ramp).
fn rms_error(d: &DecodedAudio, c: usize, hz: f32) -> f32 {
    let x = channel(d, c);
    let (from, to) = (x.len() / 4, x.len() * 3 / 4);
    let sum: f64 = (from..to)
        .map(|i| f64::from(x[i] - sine(hz, i)).powi(2))
        .sum();
    (sum / (to - from) as f64).sqrt() as f32
}

fn assert_tone(d: &DecodedAudio, c: usize, hz: f32) {
    let got = peak_hz(&channel(d, c), d.sample_rate);
    assert!(
        (got - hz).abs() <= 2.0,
        "channel {c}: {got:.2} Hz, not {hz}"
    );
    let p = peak(&channel(d, c));
    assert!((p - 0.5).abs() <= 0.1, "channel {c}: peak {p:.3}, not 0.5");
}

#[test]
fn mono_opus_decodes_to_48k_with_the_exact_length_and_tone() {
    let path = fixture(MONO);
    let d = decode_file(&path).expect("decode opus");
    assert_eq!((d.sample_rate, d.channels), (48_000, 1));
    assert_eq!(frames(&d), MONO_FRAMES);
    assert_tone(&d, 0, 440.0);
    assert_eq!(probe_sample_rate(&path).unwrap(), 48_000);
}

#[test]
fn stereo_opus_keeps_the_end_trim_and_the_channel_order() {
    let d = decode_file(&fixture(STEREO)).expect("decode opus");
    assert_eq!((d.sample_rate, d.channels), (48_000, 2));
    assert_eq!(frames(&d), STEREO_FRAMES);
    // Left is 440 Hz, right 880 Hz, as encoded.
    assert_tone(&d, 0, 440.0);
    assert_tone(&d, 1, 880.0);
}

#[test]
fn the_pre_skip_is_dropped_so_the_tone_is_in_phase_with_the_encoded_one() {
    // A pre-skip of 312 frames is 2.86 cycles of 440 Hz: dropping the wrong
    // number puts the decoded tone far from the synthesised one.
    let d = decode_file(&fixture(STEREO)).unwrap();
    let (l, r) = (rms_error(&d, 0, 440.0), rms_error(&d, 1, 880.0));
    // A 0.5 sine has an RMS of 0.354; a codec at 48 kbit/s is far closer than that.
    assert!(l < 0.05, "left differs from the encoded sine by {l}");
    assert!(r < 0.05, "right differs from the encoded sine by {r}");
}

#[test]
fn opus_in_an_ogg_file_and_an_opus_file_are_the_same_audio() {
    let dir = std::env::temp_dir().join(format!("opus-names-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let reference = decode_file(&fixture(STEREO)).unwrap();
    for name in ["take.ogg", "take.opus", "take.OGG", "take.wav", "take"] {
        let copy = dir.join(name);
        std::fs::copy(fixture(STEREO), &copy).unwrap();
        let d = decode_file(&copy).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(d.samples, reference.samples, "{name}");
        assert_eq!((d.sample_rate, d.channels), (48_000, 2), "{name}");
    }
    let bytes = std::fs::read(fixture(STEREO)).unwrap();
    assert_eq!(decode_bytes(&bytes).unwrap().samples, reference.samples);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_truncated_opus_file_never_panics_and_never_grows() {
    let bytes = std::fs::read(fixture(STEREO)).unwrap();
    let whole = decode_bytes(&bytes).unwrap();
    for cut in (0..bytes.len()).step_by(37).chain([bytes.len() - 1]) {
        if let Ok(d) = decode_bytes(&bytes[..cut]) {
            assert!(d.samples.len() <= whole.samples.len(), "cut at {cut}");
        }
    }
}

#[test]
fn damaged_opus_data_is_an_error_or_audio_of_the_same_length_never_a_panic() {
    let bytes = std::fs::read(fixture(MONO)).unwrap();
    let whole = decode_bytes(&bytes).unwrap();
    let mut errors = 0;
    for at in (0..bytes.len()).step_by(29) {
        let mut damaged = bytes.clone();
        damaged[at] ^= 0xA5;
        match decode_bytes(&damaged) {
            Ok(d) => assert!(frames(&d) <= frames(&whole), "byte {at}"),
            Err(_) => errors += 1,
        }
    }
    // A flipped byte in a page is caught by the page checksum or the header
    // checks often enough that some of these are errors.
    assert!(errors > 0, "no damaged copy was refused");
}

#[test]
fn surround_opus_is_refused_by_name() {
    let path = fixture("tones_surround_6ch_48000.opus");
    let e = decode_file(&path).unwrap_err();
    assert!(matches!(e, DecodeError::UnsupportedOpusLayout(_)), "{e:?}");
    let m = e.to_string();
    assert!(m.contains("6 channels"), "{m}");
    assert!(m.contains("mono and stereo"), "{m}");
    assert!(m.contains("WAV"), "it should say what to convert to: {m}");
}

#[test]
fn an_unknown_channel_mapping_family_is_refused_by_name() {
    let path = fixture("tones_discrete_2ch_48000.opus");
    let e = decode_file(&path).unwrap_err();
    assert!(matches!(e, DecodeError::UnsupportedOpusLayout(_)), "{e:?}");
    let m = e.to_string();
    assert!(m.contains("family 255"), "{m}");
    // Named from the bytes, not the extension.
    let bytes = std::fs::read(&path).unwrap();
    assert!(matches!(
        decode_bytes(&bytes).unwrap_err(),
        DecodeError::UnsupportedOpusLayout(_)
    ));
}
