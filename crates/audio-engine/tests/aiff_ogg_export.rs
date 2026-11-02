//! AIFF and Ogg Opus exports decode back through the app's own decoder.
//!
//! AIFF is PCM, so it must equal the WAV sample for sample. Opus is lossy,
//! so it is held to a stated tolerance instead: the decoded signal, at the
//! length of the input and with no lag, differs from the 16-bit input by
//! at most `OPUS_MAX_ERROR_DB`  relative to the input's RMS, per channel.
//! Decoding goes through `audio_decoder` (symphonia + `opus-decoder`), a
//! different implementation from the encoder's, so a file only the encoder
//! can read fails here.

use std::path::Path;

use audio_engine::{write_aiff, write_ogg_opus, write_wav, Chapter, Tags};
use tempfile::TempDir;

/// Error energy over signal energy, in dB, an Opus round trip of the
/// tones below must stay under at the default bitrate: measured -43 to
/// -45 dB for 48 kHz input.
const OPUS_MAX_ERROR_DB: f64 = -35.0;
/// For input that is resampled to 48 kHz first (44.1 kHz): measured about
/// -30 dB at 440 Hz and -21 dB at 1.2 kHz, dominated by a fractional-frame
/// offset left by rubato's whole-frame `output_delay` (0.4 of a frame at
/// 1.2 kHz is -21 dB by itself; the best whole-frame lag is -32 dB), which
/// the renderer's own resampling shares. Length and rate are exact.
const RESAMPLED_MAX_ERROR_DB: f64 = -18.0;

fn tones(rate: u32, secs: f32) -> Vec<f32> {
    let n = (rate as f32 * secs) as usize;
    let mut out = Vec::with_capacity(n * 2);
    for i in 0..n {
        let t = i as f32 / rate as f32;
        out.push((2.0 * std::f32::consts::PI * 440.0 * t).sin() * 0.6);
        out.push((2.0 * std::f32::consts::PI * 1200.0 * t).sin() * 0.3);
    }
    out
}

fn decode(path: &Path) -> audio_decoder::DecodedAudio {
    audio_decoder::decode_file(path).expect("the app's decoder reads the file")
}

#[test]
fn aiff_decodes_to_exactly_the_wav_samples() {
    let dir = TempDir::new().unwrap();
    for (rate, channels) in [(44_100u32, 2u16), (48_000, 1), (22_050, 2), (96_000, 2)] {
        let mut pcm = tones(rate, 0.25);
        if channels == 1 {
            pcm = pcm.iter().step_by(2).copied().collect();
        }
        // Include the extremes, where a wrong sign or scale shows.
        pcm[0] = 1.0;
        let last = pcm.len() - 1;
        pcm[last] = -1.0;
        let wav = dir.path().join("a.wav");
        let aiff = dir.path().join("a.aiff");
        write_wav(&pcm, rate, channels, &wav).unwrap();
        write_aiff(&pcm, rate, channels, &aiff).unwrap();
        let (w, a) = (decode(&wav), decode(&aiff));
        assert_eq!(a.sample_rate, rate);
        assert_eq!(a.channels, channels);
        assert_eq!(a.samples, w.samples, "{rate} Hz, {channels} ch");
    }
}

fn error_db(reference: &[f32], got: &[f32], channels: usize) -> Vec<f64> {
    (0..channels)
        .map(|ch| {
            let (mut sig, mut err) = (0.0f64, 0.0f64);
            for (r, g) in reference.chunks(channels).zip(got.chunks(channels)) {
                sig += f64::from(r[ch]).powi(2);
                err += f64::from(r[ch] - g[ch]).powi(2);
            }
            10.0 * (err / sig).log10()
        })
        .collect()
}

#[test]
fn ogg_opus_round_trip_keeps_length_rate_and_signal() {
    let dir = TempDir::new().unwrap();
    // 48 kHz needs no resampling; 44.1 kHz does, and comes back at 48 kHz.
    for (rate, channels) in [(48_000u32, 2u16), (44_100, 2), (48_000, 1)] {
        let mut pcm = tones(rate, 1.0);
        if channels == 1 {
            pcm = pcm.iter().step_by(2).copied().collect();
        }
        let path = dir.path().join("a.ogg");
        write_ogg_opus(&pcm, rate, channels, None, &Tags::default(), &path).unwrap();
        let got = decode(&path);
        assert_eq!(got.sample_rate, 48_000, "Opus always decodes at 48 kHz");
        assert_eq!(got.channels, channels);
        let expected_frames = (pcm.len() / channels as usize) as u64 * 48_000 / u64::from(rate);
        assert_eq!(
            (got.samples.len() / channels as usize) as u64,
            expected_frames,
            "{rate} Hz: length is the input's, padding and pre-skip cut"
        );

        // Compare against the input at 48 kHz (the reference for 44.1 kHz
        // is the same tones generated at 48 kHz: they are sines).
        let reference = {
            let mut r = tones(48_000, 1.0);
            if channels == 1 {
                r = r.iter().step_by(2).copied().collect();
            }
            r
        };
        let n = reference.len().min(got.samples.len());
        // Skip the first and last 10 ms: a resampler's edge, not the codec.
        let edge = 480 * channels as usize;
        let errs = error_db(
            &reference[edge..n - edge],
            &got.samples[edge..n - edge],
            channels as usize,
        );
        for (ch, e) in errs.iter().enumerate() {
            eprintln!("{rate} Hz {channels} ch: channel {ch} error {e:.1} dB");
            let limit = if rate == 48_000 {
                OPUS_MAX_ERROR_DB
            } else {
                RESAMPLED_MAX_ERROR_DB
            };
            assert!(*e < limit, "channel {ch}: {e:.1} dB");
        }
    }
}

#[test]
fn ogg_opus_carries_tags_and_chapters_and_honours_bitrate() {
    let dir = TempDir::new().unwrap();
    let pcm = tones(48_000, 2.0);
    let tags = Tags {
        title: Some("Episode 4".into()),
        artist: Some("Priya".into()),
        chapters: vec![Chapter {
            start_sec: 61.5,
            title: "Intro".into(),
        }],
        ..Tags::default()
    };
    let low = dir.path().join("low.ogg");
    let high = dir.path().join("high.ogg");
    write_ogg_opus(&pcm, 48_000, 2, Some(32), &tags, &low).unwrap();
    write_ogg_opus(&pcm, 48_000, 2, Some(192), &tags, &high).unwrap();
    let bytes = std::fs::read(&low).unwrap();
    for needle in [
        &b"TITLE=Episode 4"[..],
        b"ARTIST=Priya",
        b"CHAPTER001=00:01:01.500",
        b"CHAPTER001NAME=Intro",
    ] {
        assert!(bytes.windows(needle.len()).any(|w| w == needle));
    }
    assert!(std::fs::metadata(&low).unwrap().len() < std::fs::metadata(&high).unwrap().len());
    assert_eq!(decode(&low).samples.len(), decode(&high).samples.len());
}

#[test]
fn ogg_opus_refuses_surround_and_handles_empty_audio() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("x.ogg");
    assert!(write_ogg_opus(&[0.0; 600], 48_000, 6, None, &Tags::default(), &path).is_err());
    write_ogg_opus(&[], 48_000, 2, None, &Tags::default(), &path).unwrap();
    assert_eq!(decode(&path).samples.len(), 0);
}

/// The encoder is handed only bitrates Opus defines: 6 kb/s up to 256 a
/// channel. Outside that, a clear error (the young encoder is never asked).
#[test]
fn ogg_opus_bitrate_stays_inside_the_opus_range() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("b.ogg");
    let mono = vec![0.1f32; 4_800];
    let stereo = vec![0.1f32; 9_600];
    for (pcm, ch, kbps, ok) in [
        (&mono, 1, 6, true),
        (&mono, 1, 256, true),
        (&mono, 1, 257, false),
        (&mono, 1, 5, false),
        (&stereo, 2, 512, true),
        (&stereo, 2, 513, false),
    ] {
        let r = write_ogg_opus(pcm, 48_000, ch, Some(kbps), &Tags::default(), &path);
        assert_eq!(r.is_ok(), ok, "{ch} ch at {kbps} kbps: {r:?}");
        if let Err(e) = r {
            assert!(e.to_string().contains("bitrate_kbps"), "{e}");
        }
    }
}

/// Tags past what one Ogg page holds (65,024 bytes) are refused with the
/// size, before anything is encoded.
#[test]
fn ogg_opus_refuses_tags_one_page_cannot_hold() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.ogg");
    let tags = Tags {
        comment: Some("x".repeat(70_000)),
        ..Tags::default()
    };
    let e = write_ogg_opus(&[0.0; 960], 48_000, 1, None, &tags, &path).unwrap_err();
    assert!(e.to_string().contains("65,024"), "{e}");
}
