//! `stereo_to_mono` / `mono_to_stereo` must write a header that agrees
//! with the buffer they produced.
//!
//! `destructive_edit` wrote the WAV header using the *source's* channel
//! count, so a tool that changed the buffer's channel layout produced a
//! file whose header contradicted its contents. Playback reinterprets
//! the frames: half the samples under a stereo header runs at double
//! speed an octave high, and twice the samples under a mono header runs
//! at half speed an octave low. Both are total corruption, and neither
//! is visible from the sample data alone — the header is the evidence,
//! so these tests go through the dispatcher and read the file back.

use std::path::{Path, PathBuf};

use hound::WavReader;
use serde_json::json;
use tools::ToolResult;

#[path = "support/harness.rs"]
mod harness;
use harness::{write_frames, Session};

const SR: u32 = 48_000;
const FRAMES: usize = 4_800;

fn write_wav(dir: &Path, name: &str, channels: u16) -> PathBuf {
    write_frames(&dir.join(name), SR, channels, FRAMES, |n| {
        let t = n as f32 / SR as f32;
        (2.0 * std::f32::consts::PI * 440.0 * t).sin() * 0.5
    })
}

/// Run `tool` on a freshly loaded file and return the written result's
/// (channels, frame count).
fn convert(source_channels: u16, tool: &str) -> (u16, usize) {
    let mut s = Session::empty();
    let src = write_wav(s.dir.path(), "in.wav", source_channels);

    s.try_call("load", json!({ "path": src.to_string_lossy() }))
        .expect("load");

    match s.try_call(tool, json!({ "track": 0 })).expect("dispatch") {
        ToolResult::Ok(_) => {}
        ToolResult::Error(e) => panic!("`{tool}` failed: {e}"),
    }

    // The tool repointed the clip at a new content-addressed file.
    let head = s.store.head().expect("head");
    let node = s.store.get(head).expect("node");
    let written = &node.state.tracks[0].clips[0].source_path;
    let reader = WavReader::open(written).expect("open written wav");
    let spec = reader.spec();
    let total = reader.into_samples::<i16>().count();
    (spec.channels, total / spec.channels as usize)
}

/// Stereo in, mono out: the header must say mono, and the frame count
/// must be unchanged — the track's duration should not move.
#[test]
fn stereo_to_mono_writes_a_mono_header_and_keeps_the_duration() {
    let (channels, frames) = convert(2, "stereo_to_mono");
    assert_eq!(channels, 1, "header must report mono");
    assert_eq!(
        frames, FRAMES,
        "duration must not change; a stereo header here would halve it"
    );
}

/// Mono in, stereo out: header says stereo, duration unchanged.
#[test]
fn mono_to_stereo_writes_a_stereo_header_and_keeps_the_duration() {
    let (channels, frames) = convert(1, "mono_to_stereo");
    assert_eq!(channels, 2, "header must report stereo");
    assert_eq!(
        frames, FRAMES,
        "duration must not change; a mono header here would double it"
    );
}
