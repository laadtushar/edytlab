//! FLAC output must decode back to exactly the WAV output.
//!
//! FLAC is lossless, so this is an equality rather than a tolerance —
//! and it only holds if both encoders quantise the same way, which is
//! why they share one `quantise`. A tolerance-based test here would
//! pass against a FLAC that was subtly not the same master.
//!
//! Decoding is done with `symphonia`, the decoder the app itself uses,
//! rather than with `flac-codec`'s own decoder. Round-tripping a
//! library through itself would pass even if it wrote a file nothing
//! else could read, which is the failure that matters for an export.

use std::path::{Path, PathBuf};

use audio_engine::{write_flac, write_wav};
use tempfile::TempDir;

const SAMPLE_RATE: u32 = 44_100;

/// A second of stereo: a tone on the left, a quieter one on the right,
/// so a channel swap or a mono collapse is visible.
fn signal() -> Vec<f32> {
    let mut out = Vec::with_capacity(SAMPLE_RATE as usize * 2);
    for n in 0..SAMPLE_RATE as usize {
        let t = n as f32 / SAMPLE_RATE as f32;
        out.push((2.0 * std::f32::consts::PI * 440.0 * t).sin() * 0.7);
        out.push((2.0 * std::f32::consts::PI * 660.0 * t).sin() * 0.3);
    }
    out
}

fn decode(path: &Path) -> audio_decoder::DecodedAudio {
    audio_decoder::decode_file(path).expect("symphonia decodes the file")
}

fn paths(dir: &TempDir) -> (PathBuf, PathBuf) {
    (dir.path().join("out.wav"), dir.path().join("out.flac"))
}

#[test]
fn flac_decodes_to_exactly_the_same_samples_as_the_wav() {
    let tmp = TempDir::new().expect("tempdir");
    let (wav, flac) = paths(&tmp);
    let pcm = signal();

    write_wav(&pcm, SAMPLE_RATE, 2, &wav).expect("write wav");
    write_flac(&pcm, SAMPLE_RATE, 2, &flac).expect("write flac");

    let from_wav = decode(&wav);
    let from_flac = decode(&flac);

    assert_eq!(from_flac.sample_rate, from_wav.sample_rate);
    assert_eq!(from_flac.channels, from_wav.channels);
    assert_eq!(
        from_flac.samples.len(),
        from_wav.samples.len(),
        "sample count differs"
    );
    assert_eq!(
        from_flac.samples, from_wav.samples,
        "FLAC is lossless — decoded samples must be identical to the WAV, \
         not merely close. A mismatch means the two encoders quantise \
         differently."
    );
}

/// The reason to offer FLAC at all: a WAV master is too big to send.
#[test]
fn flac_is_materially_smaller_than_the_wav() {
    let tmp = TempDir::new().expect("tempdir");
    let (wav, flac) = paths(&tmp);
    let pcm = signal();

    write_wav(&pcm, SAMPLE_RATE, 2, &wav).expect("write wav");
    write_flac(&pcm, SAMPLE_RATE, 2, &flac).expect("write flac");

    let wav_len = std::fs::metadata(&wav).unwrap().len();
    let flac_len = std::fs::metadata(&flac).unwrap().len();
    assert!(
        flac_len < wav_len,
        "flac {flac_len} is not smaller than wav {wav_len}"
    );
    // Tonal material compresses well; anything near parity means the
    // encoder is effectively storing raw.
    assert!(
        (flac_len as f64) < (wav_len as f64) * 0.9,
        "flac {flac_len} vs wav {wav_len} — barely compressed"
    );
}

#[test]
fn mono_round_trips() {
    let tmp = TempDir::new().expect("tempdir");
    let (wav, flac) = paths(&tmp);
    let pcm: Vec<f32> = (0..SAMPLE_RATE)
        .map(|n| (2.0 * std::f32::consts::PI * 440.0 * n as f32 / SAMPLE_RATE as f32).sin() * 0.5)
        .collect();

    write_wav(&pcm, SAMPLE_RATE, 1, &wav).expect("write wav");
    write_flac(&pcm, SAMPLE_RATE, 1, &flac).expect("write flac");

    let from_wav = decode(&wav);
    let from_flac = decode(&flac);
    assert_eq!(from_flac.channels, 1);
    assert_eq!(from_flac.samples, from_wav.samples);
}

/// Full-scale and silence are where a quantisation mismatch shows up
/// first: clipping behaviour at ±1.0 and the sign of zero.
#[test]
fn extremes_survive_the_round_trip() {
    let tmp = TempDir::new().expect("tempdir");
    let (wav, flac) = paths(&tmp);
    let pcm: Vec<f32> = vec![1.0, -1.0, 0.0, 0.999_9, -0.999_9, 0.0, 1.5, -1.5];

    write_wav(&pcm, SAMPLE_RATE, 2, &wav).expect("write wav");
    write_flac(&pcm, SAMPLE_RATE, 2, &flac).expect("write flac");

    let from_wav = decode(&wav);
    let from_flac = decode(&flac);
    assert_eq!(
        from_flac.samples, from_wav.samples,
        "clipping or zero handling differs between the two encoders"
    );
}
