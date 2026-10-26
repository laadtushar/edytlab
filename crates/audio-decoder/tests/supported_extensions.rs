//! `SUPPORTED_EXTENSIONS` is the one list of what the app imports (#764).
//! Every entry must decode a real file, and the places that repeat the list
//! must say the same thing.

use std::path::{Path, PathBuf};

use audio_decoder::{decode_file, SUPPORTED_EXTENSIONS};

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
        "aif" | "aiff" => fixture("tone_440_mono_44100_s16.aiff"),
        other => panic!("`{other}` is in SUPPORTED_EXTENSIONS but this test has no file for it"),
    }
}

#[test]
fn every_listed_extension_decodes_a_file_with_that_extension() {
    let scratch = std::env::temp_dir().join(format!("ext-{}", std::process::id()));
    std::fs::create_dir_all(&scratch).unwrap();
    for ext in SUPPORTED_EXTENSIONS {
        // Copy under the extension itself, so the hint is the one a user's file gives.
        let copy = scratch.join(format!("tone.{ext}"));
        std::fs::copy(file_for(ext), &copy).unwrap();
        let d = decode_file(&copy).unwrap_or_else(|e| panic!(".{ext} did not decode: {e}"));
        assert!(!d.samples.is_empty(), ".{ext} decoded to nothing");
        assert!(d.sample_rate >= 8_000, ".{ext} rate {}", d.sample_rate);
    }
    let _ = std::fs::remove_dir_all(&scratch);
}

#[test]
fn the_extensions_are_lower_case_and_have_no_dot() {
    for ext in SUPPORTED_EXTENSIONS {
        assert_eq!(*ext, ext.to_ascii_lowercase());
        assert!(!ext.starts_with('.'));
    }
}

/// The window's file picker repeats the list in TypeScript. It is not
/// generated from this one, so this keeps them equal.
#[test]
fn the_file_picker_offers_exactly_the_supported_extensions() {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/desktop/src/lib/file-open.ts");
    let src = std::fs::read_to_string(&path).expect("read file-open.ts");
    let at = src
        .find("extensions: [")
        .expect("AUDIO_FILTERS has an extensions list");
    let list = &src[at + "extensions: [".len()..];
    let list = &list[..list.find(']').unwrap()];
    let mut offered: Vec<String> = list
        .split(',')
        .map(|s| s.trim().trim_matches('"').to_string())
        .filter(|s| !s.is_empty())
        .collect();
    let mut supported: Vec<String> = SUPPORTED_EXTENSIONS.iter().map(|s| s.to_string()).collect();
    offered.sort();
    supported.sort();
    assert_eq!(
        offered, supported,
        "file-open.ts AUDIO_FILTERS vs audio_decoder::SUPPORTED_EXTENSIONS"
    );
}

/// The landing screen names the formats too, by their short names.
#[test]
fn the_landing_screen_names_each_format_once() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../apps/desktop/src/components/EmptyState.tsx");
    let src = std::fs::read_to_string(&path).expect("read EmptyState.tsx");
    let line = src
        .lines()
        .find(|l| l.contains("wav ·"))
        .expect("the formats line");
    let named: Vec<&str> = line.trim().split(" · ").collect();
    for word in &named {
        // `aif` and `aiff` are one format; the screen says `aiff`.
        assert!(
            SUPPORTED_EXTENSIONS.contains(word),
            "the landing screen names `{word}`, which is not importable"
        );
    }
    for ext in SUPPORTED_EXTENSIONS.iter().filter(|e| **e != "aif") {
        assert!(
            named.contains(ext),
            "the landing screen does not name `{ext}`"
        );
    }
}
