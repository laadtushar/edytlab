//! Formats the decoder does not open (#764), and what it says about them.
//!
//! M4A/AAC is opened only by the operating system's own decoder (macOS and
//! Windows), because an AAC decoder built into the app would need a patent
//! licence; where there is none (Linux) it is refused. Opus is left out
//! because no published symphonia decodes it. Both used to come back as
//! "corrupt or unrecognised audio data" or a bare "unsupported codec", which
//! sends a person looking for a fault in a good file. They now name the format
//! and say what to do.

use std::path::{Path, PathBuf};

use audio_decoder::{decode_bytes, decode_file, supported_extensions, DecodeError};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

fn message(e: DecodeError) -> String {
    assert!(
        matches!(e, DecodeError::UnsupportedFormat(_)),
        "expected UnsupportedFormat, got {e:?}"
    );
    e.to_string()
}

// Linux has no system AAC decoder, so these three refuse M4A. On macOS and
// Windows the same files decode (tests/formats.rs).

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
#[test]
fn an_m4a_file_is_named_not_called_corrupt() {
    let m = message(decode_file(&fixture("tone_440_mono_44100.m4a")).unwrap_err());
    assert!(m.contains("M4A"), "{m}");
    assert!(m.contains("not supported"), "{m}");
    assert!(m.contains("WAV"), "it should say what to convert to: {m}");
}

#[test]
fn an_opus_file_is_named_not_called_unsupported_codec() {
    let m = message(decode_file(&fixture("tone_440_mono_48000.opus")).unwrap_err());
    assert!(m.contains("Opus"), "{m}");
    assert!(m.contains("not supported"), "{m}");
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
#[test]
fn the_name_is_found_from_the_bytes_not_the_extension() {
    // A phone voice memo renamed `.wav`, and bytes handed over with no name.
    let m4a = std::fs::read(fixture("tone_440_mono_44100.m4a")).unwrap();
    assert!(message(decode_bytes(&m4a).unwrap_err()).contains("M4A"));
    let renamed = std::env::temp_dir().join(format!("renamed-{}.wav", std::process::id()));
    std::fs::write(&renamed, &m4a).unwrap();
    let got = decode_file(&renamed);
    let _ = std::fs::remove_file(&renamed);
    assert!(message(got.unwrap_err()).contains("M4A"));
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
#[test]
fn probing_the_rate_of_an_m4a_says_the_same() {
    // An Opus file's header does carry a rate, so the probe reads it; `load`
    // refuses the file at the decode, before any clip names it.
    let m =
        message(audio_decoder::probe_sample_rate(&fixture("tone_440_mono_44100.m4a")).unwrap_err());
    assert!(m.contains("M4A"), "{m}");
}

#[test]
fn a_file_that_is_just_garbage_is_not_given_a_format_name() {
    // It was "unsupported codec" before and stays that: only bytes that
    // really are M4A/MP4 or Ogg Opus are named.
    let e = decode_bytes(b"this is not audio at all, just words").unwrap_err();
    assert!(
        matches!(e, DecodeError::UnsupportedCodec | DecodeError::Corrupt),
        "{e:?}"
    );
}

#[test]
fn the_supported_list_leaves_out_what_does_not_decode() {
    let listed = supported_extensions();
    // A bare `.aac` (raw ADTS), `.mp4` and Opus are not routed to any decoder.
    for ext in ["aac", "mp4", "opus"] {
        assert!(
            !listed.contains(&ext),
            "{ext} does not decode, so it must not be listed"
        );
    }
    // M4A is listed exactly where the system decodes it.
    assert_eq!(
        listed.contains(&"m4a"),
        cfg!(any(target_os = "macos", target_os = "windows")),
        "m4a is listed where the operating system decodes it, and nowhere else"
    );
}
