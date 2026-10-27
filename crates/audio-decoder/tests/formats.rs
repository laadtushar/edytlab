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

// --- M4A/AAC-LC through the operating system's decoder ----------------------
//
// AudioToolbox on macOS and Media Foundation on Windows (#764). Linux has no
// system decoder and refuses the files (`tests/unsupported.rs`). The tolerances
// are the lossy ones plus a wider length slack: the two decoders differ in
// whether they trim the encoder's priming samples (about 1,024 to 2,112
// frames, up to 48 ms), so the length is checked to 120 ms.

#[cfg(any(target_os = "macos", target_os = "windows"))]
mod os_aac {
    use super::*;
    use audio_decoder::decode_bytes;

    const AAC_LENGTH_SLACK_SECS: f32 = 0.12;

    fn assert_aac_length_and_level(d: &DecodedAudio) {
        let secs = (d.samples.len() / d.channels as usize) as f32 / d.sample_rate as f32;
        assert!(
            (secs - DURATION_SECS).abs() <= AAC_LENGTH_SLACK_SECS,
            "length {secs:.4} s, encoded {DURATION_SECS} s (slack {AAC_LENGTH_SLACK_SECS} s)"
        );
        for c in 0..d.channels as usize {
            let p = peak(&channel(d, c));
            assert!(
                (p - 0.5).abs() <= LOSSY_LEVEL_TOLERANCE,
                "channel {c}: peak {p:.3}, encoded 0.5 (tolerance {LOSSY_LEVEL_TOLERANCE})"
            );
        }
        assert!(
            d.samples.iter().all(|s| s.is_finite() && s.abs() <= 1.5),
            "decoded samples must be finite and near full scale"
        );
    }

    #[test]
    fn m4a_aac_lc_mono_decodes_to_the_encoded_tone() {
        let path = fixture("tone_440_mono_44100.m4a");
        let d = decode_file(&path).expect("decode m4a");
        assert_tones(
            &d,
            &Expect {
                rate: 44_100,
                tones_hz: &[440.0],
            },
        );
        assert_aac_length_and_level(&d);
        assert_eq!(probe_sample_rate(&path).unwrap(), 44_100);
    }

    #[test]
    fn m4a_aac_lc_stereo_keeps_each_channel_and_its_rate() {
        let path = fixture("tones_440_880_stereo_48000.m4a");
        let d = decode_file(&path).expect("decode m4a");
        assert_tones(
            &d,
            &Expect {
                rate: 48_000,
                tones_hz: &[440.0, 880.0],
            },
        );
        assert_aac_length_and_level(&d);
        assert_eq!(probe_sample_rate(&path).unwrap(), 48_000);
    }

    /// A voice memo renamed `.wav`, and bytes handed over with no name, are
    /// still found to be M4A from their first bytes.
    #[test]
    fn m4a_is_recognised_by_its_bytes_not_its_extension() {
        let bytes = std::fs::read(fixture("tone_440_mono_44100.m4a")).unwrap();
        let d = decode_bytes(&bytes).expect("decode m4a bytes");
        assert_eq!((d.sample_rate, d.channels), (44_100, 1));
        assert_aac_length_and_level(&d);

        let renamed = std::env::temp_dir().join(format!("renamed-m4a-{}.wav", std::process::id()));
        std::fs::write(&renamed, &bytes).unwrap();
        let got = decode_file(&renamed);
        let _ = std::fs::remove_file(&renamed);
        let d = got.expect("decode a renamed m4a");
        assert_eq!((d.sample_rate, d.channels), (44_100, 1));
    }

    /// Bytes that start like an M4A and stop short are an error, not a hang or
    /// a panic. The system decoder's answer is mapped, whichever it is.
    #[test]
    fn a_truncated_m4a_is_an_error_not_a_crash() {
        let bytes = std::fs::read(fixture("tone_440_mono_44100.m4a")).unwrap();
        let err = decode_bytes(&bytes[..64]).expect_err("64 bytes are not a file");
        assert!(
            matches!(
                err,
                audio_decoder::DecodeError::Corrupt
                    | audio_decoder::DecodeError::UnsupportedCodec
                    | audio_decoder::DecodeError::NoAudioTrack
                    | audio_decoder::DecodeError::OsDecoderUnavailable(_)
            ),
            "{err:?}"
        );
    }
}

// --- HE-AAC on macOS ---------------------------------------------------------
//
// ffmpeg has no HE-AAC encoder, but macOS ships `afconvert`, so these files are
// made when the test runs, from a generated WAV, and skipped (loudly) if
// `afconvert` is not there. HE-AAC doubles the rate with spectral band
// replication: the file may declare the core rate, and a client format that
// took it would hand back a half-rate signal (the tone an octave low).

#[cfg(target_os = "macos")]
mod he_aac_macos {
    use super::*;
    use std::process::Command;

    /// Write `secs` of a tone per channel (440 Hz, then 880 Hz) as a 16-bit WAV.
    fn write_wav(path: &Path, rate: u32, channels: u16, secs: f32) {
        let mut w = hound::WavWriter::create(
            path,
            hound::WavSpec {
                channels,
                sample_rate: rate,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            },
        )
        .unwrap();
        for i in 0..(rate as f32 * secs) as usize {
            for c in 0..channels {
                let hz = 440.0 * (c as f32 + 1.0);
                let x = 0.5 * (2.0 * std::f32::consts::PI * hz * i as f32 / rate as f32).sin();
                w.write_sample((x * 32_767.0) as i16).unwrap();
            }
        }
        w.finalize().unwrap();
    }

    /// Make an HE-AAC file with `afconvert`; `None` (skip) if it is not installed.
    fn make(tag: &str, codec: &str, bps: u32, rate: u32, channels: u16) -> Option<PathBuf> {
        let dir = std::env::temp_dir().join(format!("he-aac-{}-{tag}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let wav = dir.join("in.wav");
        let m4a = dir.join("out.m4a");
        write_wav(&wav, rate, channels, 1.5);
        let out = match Command::new("afconvert")
            .args(["-f", "m4af", "-d", codec, "-b", &bps.to_string()])
            .arg(&wav)
            .arg(&m4a)
            .output()
        {
            Ok(o) => o,
            Err(e) => {
                eprintln!("SKIPPED {tag}: afconvert cannot be run: {e}");
                return None;
            }
        };
        assert!(
            out.status.success(),
            "afconvert {codec} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        Some(m4a)
    }

    fn check(tag: &str, codec: &str, bps: u32, rate: u32, channels: u16) {
        let Some(path) = make(tag, codec, bps, rate, channels) else {
            return;
        };
        let d = decode_file(&path).expect("decode HE-AAC");
        let frames = d.samples.len() / d.channels as usize;
        eprintln!(
            "HE_AAC {tag}: rate {} channels {} frames {frames}",
            d.sample_rate, d.channels
        );
        assert_eq!(d.sample_rate, rate, "{tag}: full rate, not the core rate");
        assert_eq!(d.channels, channels, "{tag}: channels");
        let secs = frames as f32 / rate as f32;
        assert!((secs - 1.5).abs() <= 0.15, "{tag}: length {secs:.3} s");
        for c in 0..channels as usize {
            let hz = 440.0 * (c as f32 + 1.0);
            let got = peak_hz(&channel(&d, c), rate);
            assert!(
                (got - hz).abs() <= FREQ_TOLERANCE_HZ,
                "{tag}: channel {c} found {got:.2} Hz, encoded {hz} Hz"
            );
            let p = peak(&channel(&d, c));
            assert!(
                (p - 0.5).abs() <= 0.15,
                "{tag}: channel {c} peak {p:.3}, encoded 0.5"
            );
        }
        assert_eq!(probe_sample_rate(&path).unwrap(), rate, "{tag}: probe");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn he_aac_v1_stereo_44100_decodes_at_full_rate() {
        check("v1-stereo-44100", "aach", 64_000, 44_100, 2);
    }

    #[test]
    fn he_aac_v1_mono_48000_decodes_at_full_rate() {
        check("v1-mono-48000", "aach", 32_000, 48_000, 1);
    }

    #[test]
    fn he_aac_v2_stereo_44100_decodes_at_full_rate() {
        check("v2-stereo-44100", "aacp", 32_000, 44_100, 2);
    }
}

// TEMPORARY: reports the frame counts of both fixtures on each OS (CI is the
// measuring tool). Removed once the priming is settled.
#[cfg(any(target_os = "macos", target_os = "windows"))]
#[test]
fn zz_diag_frame_counts() {
    let mut report = String::new();
    for name in ["tone_440_mono_44100.m4a", "tones_440_880_stereo_48000.m4a"] {
        let d = decode_file(&fixture(name)).unwrap();
        report.push_str(&format!(
            "\nDIAG {name}: rate {} channels {} frames {}",
            d.sample_rate,
            d.channels,
            d.samples.len() / d.channels as usize
        ));
    }
    panic!("{report}");
}
