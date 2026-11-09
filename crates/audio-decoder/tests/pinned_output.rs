//! What each fixture decodes to, pinned: the rate, the channel count, the
//! number of frames and the samples themselves. The other tests check that a
//! decode is the right tone at about the right length; this one fails on any
//! change at all, so a new decoder (symphonia 0.5.5 to 0.6.1 was one) cannot
//! change an import without a test saying so. The values were measured with
//! symphonia 0.5.5, before the move to 0.6.1, which decodes them the same.
//!
//! The samples are compared as an FNV-1a hash of their bits. WAV, AIFF and
//! FLAC are integers scaled to `f32`, which is exact, so their hash is checked
//! everywhere. MP3, Vorbis and Opus are decoded with floating point, whose
//! tables are built at run time from the platform's maths library, so their
//! hash is checked on x86-64 Linux, where it was measured; on every platform
//! their rate, channels and length are, and what is said of their range
//! below.
//!
//! Three of these pin a behaviour symphonia 0.6 changed and `src/compat.rs`
//! keeps: an MP3 with a LAME tag keeps its encoder delay and padding (0.6
//! trims them when gapless playback is on, its default), a Vorbis file has no
//! audio for its first packet and none past full scale, and an AIFF file cut
//! short decodes as far as it goes.

mod support;

use audio_decoder::{decode_bytes, decode_file, DecodedAudio};
use support::fixture;

/// A decode's expected shape and samples.
struct Pin {
    file: &'static str,
    rate: u32,
    channels: u16,
    frames: usize,
    /// FNV-1a 64 of every sample's bits, little-endian, in order.
    hash: u64,
    /// The hash is the same on every platform (integer PCM scaled to `f32`).
    exact_everywhere: bool,
}

/// FNV-1a, 64-bit: small, dependency-free and order-sensitive.
fn fnv1a(samples: &[f32]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for s in samples {
        for b in s.to_bits().to_le_bytes() {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    h
}

fn check(pin: &Pin, d: &DecodedAudio) {
    let frames = d.samples.len() / usize::from(d.channels.max(1));
    let got = (d.sample_rate, d.channels, frames);
    assert_eq!(
        got,
        (pin.rate, pin.channels, pin.frames),
        "{}: (rate, channels, frames)",
        pin.file
    );
    assert_eq!(
        d.samples.len(),
        frames * usize::from(d.channels),
        "{}: whole frames",
        pin.file
    );
    let hash_measured_here = cfg!(all(target_os = "linux", target_arch = "x86_64"));
    if pin.exact_everywhere || hash_measured_here {
        assert_eq!(
            fnv1a(&d.samples),
            pin.hash,
            "{}: the decoded samples changed (hash {:#018x})",
            pin.file,
            fnv1a(&d.samples)
        );
    }
}

/// The build script's golden files (`build.rs`): 1 s of 440 Hz at -3 dBFS.
fn golden(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("OUT_DIR")).join(name)
}

const PINS: &[Pin] = &[
    Pin {
        file: "tone_440_mono_44100_s16.aiff",
        rate: 44_100,
        channels: 1,
        frames: 22_050,
        hash: 0xe50e_39e5_a7df_766d,
        exact_everywhere: true,
    },
    Pin {
        file: "tones_440_880_stereo_48000_s24.aiff",
        rate: 48_000,
        channels: 2,
        frames: 24_000,
        hash: 0x097e_c443_22c9_daf5,
        exact_everywhere: true,
    },
    Pin {
        file: "tone_440_mono_44100.flac",
        rate: 44_100,
        channels: 1,
        frames: 22_050,
        hash: 0xe50e_39e5_a7df_766d,
        exact_everywhere: true,
    },
    // Vorbis: 30 and 256 frames past the 0.5 s encoded (no gapless trim), and
    // no audio for the first packet.
    Pin {
        file: "tone_440_mono_44100.ogg",
        rate: 44_100,
        channels: 1,
        frames: 22_080,
        hash: 0x8315_c839_0982_f794,
        exact_everywhere: false,
    },
    Pin {
        file: "tones_440_880_stereo_48000.ogg",
        rate: 48_000,
        channels: 2,
        frames: 24_256,
        hash: 0x86b0_35a7_757b_a550,
        exact_everywhere: false,
    },
    Pin {
        file: "square_220_full_scale_mono_44100.ogg",
        rate: 44_100,
        channels: 1,
        frames: 13_312,
        hash: 0x4917_1605_d3e8_3c1e,
        exact_everywhere: false,
    },
    // MP3 with a LAME tag: 21 MPEG frames of 1,152 after the Info frame, the
    // encoder delay and padding the tag declares kept (no gapless trim).
    Pin {
        file: "tone_440_mono_44100.mp3",
        rate: 44_100,
        channels: 1,
        frames: 24_192,
        hash: 0xe815_6a4f_086a_0514,
        exact_everywhere: false,
    },
    // Opus is trimmed by `opus.rs` itself (RFC 7845), not by symphonia.
    Pin {
        file: "tone_440_mono_48000.opus",
        rate: 48_000,
        channels: 1,
        frames: 24_000,
        hash: 0x14e6_ee47_de19_3205,
        exact_everywhere: false,
    },
    Pin {
        file: "tones_440_880_stereo_48000.opus",
        rate: 48_000,
        channels: 2,
        frames: 46_560,
        hash: 0x5a1f_afee_be49_0cb8,
        exact_everywhere: false,
    },
];

#[test]
fn every_fixture_decodes_to_its_pinned_samples() {
    for pin in PINS {
        let path = fixture(pin.file);
        check(pin, &decode_file(&path).unwrap());
        // The in-memory path decodes the same.
        check(pin, &decode_bytes(&std::fs::read(&path).unwrap()).unwrap());
    }
}

#[test]
fn the_golden_wav_and_mp3_decode_to_their_pinned_samples() {
    let wav = Pin {
        file: "sine_440hz_1s.wav",
        rate: 44_100,
        channels: 1,
        frames: 44_100,
        hash: 0xc0bd_c5a5_1fb4_1e73,
        exact_everywhere: true,
    };
    check(&wav, &decode_file(&golden(wav.file)).unwrap());
    // 39 MPEG frames of 1,152, no Xing tag; its decode goes past full scale
    // and is clamped (below).
    let mp3 = Pin {
        file: "sine_440hz_1s.mp3",
        rate: 44_100,
        channels: 1,
        frames: 44_928,
        hash: 0x3e6f_5dda_afa2_cd2e,
        exact_everywhere: false,
    };
    check(&mp3, &decode_file(&golden(mp3.file)).unwrap());
}

/// MP3 and Vorbis decode to [-1, 1]: a sample the codec puts past full scale
/// is clamped there, as symphonia 0.5.5's decoders did. These two files go
/// past it, so each holds a sample at exactly full scale.
#[test]
fn mp3_and_vorbis_are_clamped_to_full_scale() {
    for path in [
        golden("sine_440hz_1s.mp3"),
        fixture("square_220_full_scale_mono_44100.ogg"),
    ] {
        let d = decode_file(&path).unwrap();
        assert!(
            d.samples.iter().all(|s| (-1.0..=1.0).contains(s)),
            "{}: a sample past full scale",
            path.display()
        );
        assert!(
            d.samples.iter().any(|s| s.abs() == 1.0),
            "{}: no sample at full scale, so nothing here was clamped",
            path.display()
        );
    }
}

/// An AIFF file cut short inside its sound data decodes the whole frames it
/// holds, from `decode_file`, `decode_bytes` and `probe_sample_rate` alike.
#[test]
fn an_aiff_cut_short_decodes_as_far_as_it_goes() {
    let whole = std::fs::read(fixture("tones_440_880_stereo_48000_s24.aiff")).unwrap();
    let cut = &whole[..whole.len() * 6 / 10];
    let pin = Pin {
        file: "tones_440_880_stereo_48000_s24.aiff, cut to 60%",
        rate: 48_000,
        channels: 2,
        frames: 14_396,
        hash: 0xe6f9_a4db_c965_765d,
        exact_everywhere: true,
    };
    check(&pin, &decode_bytes(cut).unwrap());
    let dir = std::env::temp_dir().join(format!("audio-decoder-pinned-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("cut.aiff");
    std::fs::write(&path, cut).unwrap();
    check(&pin, &decode_file(&path).unwrap());
    assert_eq!(audio_decoder::probe_sample_rate(&path).unwrap(), 48_000);
    std::fs::remove_dir_all(&dir).unwrap();
}
