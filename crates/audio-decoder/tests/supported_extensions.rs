//! `SUPPORTED_EXTENSIONS` and `OS_DECODER_EXTENSIONS` are the lists of what the
//! app imports (#764): the first on every platform, the second (M4A) only where
//! the operating system has a decoder. Every entry of `supported_extensions()`
//! must decode a real file on the platform the test runs on, and the places that
//! repeat the lists must say the same thing.

use std::path::{Path, PathBuf};

use audio_decoder::{
    decode_file, supported_extensions, OS_DECODER_AVAILABLE, OS_DECODER_EXTENSIONS,
    SUPPORTED_EXTENSIONS,
};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

/// One file for each extension. The WAV and MP3 are made by `build.rs`.
fn file_for(ext: &str) -> PathBuf {
    let out = PathBuf::from(env!("OUT_DIR"));
    match ext {
        "wav" => out.join("sine_440hz_1s.wav"),
        "mp3" => out.join("sine_440hz_1s.mp3"),
        "flac" => fixture("tone_440_mono_44100.flac"),
        "ogg" => fixture("tone_440_mono_44100.ogg"),
        "opus" => fixture("tone_440_mono_48000.opus"),
        "aif" | "aiff" => fixture("tone_440_mono_44100_s16.aiff"),
        "m4a" => fixture("tone_440_mono_44100.m4a"),
        other => panic!("`{other}` is in SUPPORTED_EXTENSIONS but this test has no file for it"),
    }
}

#[test]
fn every_listed_extension_decodes_a_file_with_that_extension() {
    let scratch = std::env::temp_dir().join(format!("ext-{}", std::process::id()));
    std::fs::create_dir_all(&scratch).unwrap();
    for ext in supported_extensions() {
        // Copy under the extension itself, so the hint is the one a user's file gives.
        let copy = scratch.join(format!("tone.{ext}"));
        std::fs::copy(file_for(ext), &copy).unwrap();
        let d = decode_file(&copy).unwrap_or_else(|e| panic!(".{ext} did not decode: {e}"));
        assert!(!d.samples.is_empty(), ".{ext} decoded to nothing");
        assert!(d.sample_rate >= 8_000, ".{ext} rate {}", d.sample_rate);
    }
    let _ = std::fs::remove_dir_all(&scratch);
}

/// A platform that lists an OS-decoded extension must have a fixture for it, so
/// the macOS and Windows legs of CI decode a real file for each.
#[test]
fn every_os_decoded_extension_has_a_fixture_on_every_platform() {
    for ext in OS_DECODER_EXTENSIONS {
        assert!(file_for(ext).exists(), "no fixture for .{ext}");
    }
}

#[test]
fn the_extensions_are_lower_case_and_have_no_dot() {
    for ext in supported_extensions() {
        assert_eq!(ext, ext.to_ascii_lowercase());
        assert!(!ext.starts_with('.'));
    }
}

/// M4A is on the list where the system decodes it, and only there: the shared
/// list never holds it, and a platform without a system decoder never offers it.
#[test]
fn m4a_is_listed_on_exactly_the_platforms_that_decode_it() {
    assert_eq!(
        OS_DECODER_AVAILABLE,
        cfg!(any(target_os = "macos", target_os = "windows"))
    );
    for ext in OS_DECODER_EXTENSIONS {
        assert!(
            !SUPPORTED_EXTENSIONS.contains(ext),
            "{ext} is only opened by the system's decoder, so it is not in the shared list"
        );
        assert_eq!(
            supported_extensions().contains(ext),
            OS_DECODER_AVAILABLE,
            "{ext} on this platform"
        );
    }
    assert!(OS_DECODER_EXTENSIONS.contains(&"m4a"));
    // The `aac` extension (raw ADTS) and `mp4` are not routed to the system.
    for ext in ["aac", "mp4", "opus"] {
        assert!(!OS_DECODER_EXTENSIONS.contains(&ext), "{ext}");
    }
}

/// The strings of `export const NAME = [ ... ];` in a TypeScript source.
fn ts_list(src: &str, name: &str) -> Vec<String> {
    let marker = format!("export const {name} = [");
    let at = src
        .find(&marker)
        .unwrap_or_else(|| panic!("importFormats.ts has no `{name}` list"));
    let list = &src[at + marker.len()..];
    let list = &list[..list.find(']').unwrap()];
    list.split(',')
        .map(|s| s.trim().trim_matches('"').to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn read_window_source(relative: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../apps/desktop/src")
        .join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// The window's file picker repeats the lists in TypeScript
/// (`lib/importFormats.ts`). They are not generated from these, so this keeps
/// them equal.
#[test]
fn the_file_picker_offers_exactly_the_supported_extensions() {
    let src = read_window_source("lib/importFormats.ts");
    let sorted = |mut v: Vec<String>| {
        v.sort();
        v
    };
    assert_eq!(
        sorted(ts_list(&src, "BASE_EXTENSIONS")),
        sorted(SUPPORTED_EXTENSIONS.iter().map(|s| s.to_string()).collect()),
        "importFormats.ts BASE_EXTENSIONS vs audio_decoder::SUPPORTED_EXTENSIONS"
    );
    assert_eq!(
        sorted(ts_list(&src, "OS_DECODER_EXTENSIONS")),
        sorted(
            OS_DECODER_EXTENSIONS
                .iter()
                .map(|s| s.to_string())
                .collect()
        ),
        "importFormats.ts OS_DECODER_EXTENSIONS vs audio_decoder::OS_DECODER_EXTENSIONS"
    );
    // The picker reads the platform's list from there and nowhere else.
    let picker = read_window_source("lib/file-open.ts");
    assert!(
        picker.contains("importExtensions()"),
        "file-open.ts must take its extensions from importFormats.ts"
    );
    assert!(
        !picker.contains("extensions: ["),
        "file-open.ts must not keep a list of its own"
    );
}

/// The window decides from the user agent whether the system decodes M4A.
/// Only macOS and Windows may say yes, as the decoder does.
#[test]
fn the_window_offers_m4a_only_on_the_platforms_the_decoder_has_a_system_decoder_for() {
    let src = read_window_source("lib/importFormats.ts");
    assert!(
        src.contains("/Mac|Win/.test(platform)"),
        "importFormats.ts must key M4A on macOS and Windows"
    );
}

/// The landing screen names the formats too, by their short names.
#[test]
fn the_landing_screen_names_each_format_once() {
    let src = read_window_source("lib/importFormats.ts");
    let named = ts_list(&src, "BASE_FORMAT_NAMES");
    for word in &named {
        // `aif` and `aiff` are one format; the screen says `aiff`.
        assert!(
            SUPPORTED_EXTENSIONS.contains(&word.as_str()),
            "the landing screen names `{word}`, which is not importable"
        );
    }
    for ext in SUPPORTED_EXTENSIONS.iter().filter(|e| **e != "aif") {
        assert!(
            named.iter().any(|n| n == ext),
            "the landing screen does not name `{ext}`"
        );
    }
    let landing = read_window_source("components/EmptyState.tsx");
    assert!(
        landing.contains("importFormatNames()"),
        "EmptyState.tsx must list the formats from importFormats.ts"
    );
}
