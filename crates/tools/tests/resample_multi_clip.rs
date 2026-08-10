//! `resample_track` writes a file at a rate its source never had, so it
//! could not use the shared destructive-edit path and carried its own
//! copy instead.
//!
//! That copy stopped at `clips.first()`. On a track split by an interior
//! cut it converted the head and dropped everything after the join — the
//! resampled track came back a fraction of its length, with no error to
//! say most of it had gone.

use std::path::{Path, PathBuf};

use hound::{SampleFormat, WavReader, WavSpec, WavWriter};
use serde_json::{json, Value};
use tempfile::TempDir;
use tools::{ToolContext, ToolDispatcher, ToolResult};

const SAMPLE_RATE: u32 = 8_000;
const SOURCE_FRAMES: usize = 8_000;

fn ok(result: ToolResult) -> Value {
    match result {
        ToolResult::Ok(v) => v,
        ToolResult::Error(msg) => panic!("expected Ok, got Error({msg})"),
    }
}

fn write_tone_wav(dir: &Path) -> PathBuf {
    let path = dir.join("in.wav");
    let spec = WavSpec {
        channels: 1,
        sample_rate: SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut writer = WavWriter::create(&path, spec).expect("wav writer");
    for n in 0..SOURCE_FRAMES {
        let t = n as f32 / SAMPLE_RATE as f32;
        let s = (2.0 * std::f32::consts::PI * 440.0 * t).sin() * 0.4;
        let q = (s * 32_768.0)
            .round()
            .clamp(i16::MIN as f32, i16::MAX as f32) as i16;
        writer.write_sample(q).unwrap();
    }
    writer.finalize().unwrap();
    path
}

/// Resampling a split track has to convert every clip, not just the head.
///
/// The assertion is on the session state rather than the render, because
/// the engine resamples every track back to the *project* rate on the way
/// out — so a rendered file looks the same whichever clips were converted.
/// What `resample_track` actually changes is the clip: its source file,
/// that file's rate, and its length in frames.
#[test]
fn resample_covers_every_clip_of_a_split_track() {
    let tmp = TempDir::new().expect("tempdir");
    let mut store = session::Store::open(tmp.path()).expect("open store");
    let mut engine = audio_engine::Engine::new();
    let dispatcher = ToolDispatcher::default_dispatcher();
    let src = write_tone_wav(tmp.path());

    let mut clipboard: Option<Vec<f32>> = None;
    let mut ctx = ToolContext {
        store: &mut store,
        engine: &mut engine,
        user_message: "",
        clipboard: &mut clipboard,
    };

    ok(dispatcher
        .invoke("load", json!({ "path": src.to_string_lossy() }), &mut ctx)
        .unwrap());
    // Interior cut: 7000 frames left, in two clips.
    ok(dispatcher
        .invoke(
            "cut_range",
            json!({ "track": 0, "start_sample": 2000, "end_sample": 3000 }),
            &mut ctx,
        )
        .unwrap());

    ok(dispatcher
        .invoke(
            "resample_track",
            json!({ "track": 0, "target_sample_rate": SAMPLE_RATE / 2 }),
            &mut ctx,
        )
        .unwrap());

    let head = ctx.store.head().expect("head");
    let node = ctx.store.get(head).expect("get head");
    let clips = &node.state.tracks[0].clips;

    assert_eq!(
        clips.len(),
        1,
        "the converted buffer is the whole track, so it collapses to one clip"
    );

    // 7000 source frames at half the rate. The old code converted the
    // 2000-frame head alone and left the 5000-frame tail pointing at the
    // original file, at the original rate.
    let len = clips[0].length as usize;
    assert!(
        (3_400..=3_600).contains(&len),
        "the whole 7000-frame timeline should resample to ~3500 frames, got {len}"
    );

    let written = WavReader::open(&clips[0].source_path).expect("open derived");
    assert_eq!(
        written.spec().sample_rate,
        SAMPLE_RATE / 2,
        "the derived file should carry the requested rate"
    );
    assert_eq!(
        written.duration() as usize,
        len,
        "the clip length must match the file it points at"
    );
}
