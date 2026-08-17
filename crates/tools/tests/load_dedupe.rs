//! The same audio is stored once, wherever it came from (#160).
//!
//! `ensure_streamable_wav` transcodes a non-streamable source (an MP3,
//! say) into a content-addressed WAV under `derived/`. The name used to
//! be hashed over the *source path* as well as the samples, so the same
//! recording imported from two locations — a copy, a move, a
//! re-download — produced two different names and two full copies on
//! disk. Roughly 55 MB per re-import for a five-minute stereo file, and
//! it compounds with the unbounded growth in #98.
//!
//! That defeats content addressing for exactly the case it exists to
//! serve. Two files whose decoded samples, sample rate and channel count
//! all match *are* the same audio for every purpose this store has.
//!
//! The tests below pin both directions, because dropping the path from a
//! hash is only safe if identical audio still collides and different
//! audio still does not.

use std::path::Path;

use hound::{SampleFormat, WavSpec, WavWriter};
use serde_json::{json, Value};
use tempfile::TempDir;
use tools::{ToolContext, ToolDispatcher, ToolResult};

const SAMPLE_RATE: u32 = 44_100;

fn tone(freq: f32) -> Vec<f32> {
    (0..(SAMPLE_RATE as usize / 4))
        .map(|n| {
            let t = n as f32 / SAMPLE_RATE as f32;
            (2.0 * std::f32::consts::PI * freq * t).sin() * 0.4
        })
        .collect()
}

/// A FLAC source, because that is what actually exercises the path
/// under test. `WavStreamReader::open` accepts any WAV hound can read —
/// it never rejects on bit depth or sample format — so only a non-WAV
/// input makes `ensure_streamable_wav` transcode. (I first wrote these
/// against a 32-bit float WAV and every case produced zero derived
/// files, which is how I found that out.)
fn write_flac(path: &Path, freq: f32) {
    audio_engine::write_flac(&tone(freq), SAMPLE_RATE, 1, path).expect("flac");
}

/// A plain 16-bit PCM WAV — streamable, so it should never be copied.
fn write_wav(path: &Path, freq: f32) {
    let spec = WavSpec {
        channels: 1,
        sample_rate: SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut w = WavWriter::create(path, spec).expect("wav writer");
    for s in tone(freq) {
        w.write_sample((s * 32_767.0) as i16).unwrap();
    }
    w.finalize().unwrap();
}

fn ok(r: ToolResult) -> Value {
    match r {
        ToolResult::Ok(v) => v,
        ToolResult::Error(m) => panic!("expected Ok, got Error({m})"),
    }
}

/// Load `path` into a fresh session and return how many files ended up
/// in that project's `derived/` directory.
fn derived_count(dir: &Path) -> usize {
    let d = dir.join("derived");
    if !d.exists() {
        return 0;
    }
    std::fs::read_dir(d)
        .expect("read derived")
        .flatten()
        .filter(|e| e.path().is_file())
        .count()
}

fn load_both(a: &Path, b: &Path, project: &Path) {
    let mut store = session::Store::open(project).expect("open store");
    let mut engine = audio_engine::Engine::new();
    let dispatcher = ToolDispatcher::default_dispatcher();
    let mut clipboard: Option<Vec<f32>> = None;
    let mut ctx = ToolContext {
        store: &mut store,
        engine: &mut engine,
        user_message: "",
        clipboard: &mut clipboard,
    };
    for p in [a, b] {
        ok(dispatcher
            .invoke("load", json!({ "path": p.to_string_lossy() }), &mut ctx)
            .unwrap());
    }
}

/// **The bug.** Identical audio at two paths must land on one file.
#[test]
fn the_same_audio_from_two_paths_is_stored_once() {
    let tmp = TempDir::new().expect("tempdir");
    let a = tmp.path().join("original.flac");
    let b = tmp.path().join("a-copy-elsewhere.flac");
    // Byte-identical content at two paths.
    write_flac(&a, 440.0);
    std::fs::copy(&a, &b).expect("copy");

    load_both(&a, &b, tmp.path());

    assert_eq!(
        derived_count(tmp.path()),
        1,
        "the same audio imported twice should transcode to one file; \
         hashing the source path is what used to make it two"
    );
}

/// The property that made dropping the path safe: different audio must
/// still get different names.
#[test]
fn different_audio_still_gets_different_files() {
    let tmp = TempDir::new().expect("tempdir");
    let a = tmp.path().join("tone-a.flac");
    let b = tmp.path().join("tone-b.flac");
    write_flac(&a, 440.0);
    write_flac(&b, 660.0);

    load_both(&a, &b, tmp.path());

    assert_eq!(
        derived_count(tmp.path()),
        2,
        "two different recordings must not collide onto one name"
    );
}

/// Re-loading the very same path was already idempotent and must stay so.
#[test]
fn reloading_one_path_is_still_idempotent() {
    let tmp = TempDir::new().expect("tempdir");
    let a = tmp.path().join("once.flac");
    write_flac(&a, 440.0);

    load_both(&a, &a, tmp.path());

    assert_eq!(derived_count(tmp.path()), 1);
}

/// A source that is already streamable is used in place and never
/// transcoded, so it contributes nothing to `derived/` at all.
#[test]
fn an_already_streamable_source_is_not_copied() {
    let tmp = TempDir::new().expect("tempdir");
    let a = tmp.path().join("plain.wav");
    write_wav(&a, 440.0);

    load_both(&a, &a, tmp.path());

    assert_eq!(
        derived_count(tmp.path()),
        0,
        "a 16-bit PCM WAV streams directly; transcoding it would be pure waste"
    );
}
