//! `eq`, `compressor` and `noise_reduction` bypass `destructive_edit` and
//! do their own decode-edit-write.
//!
//! Each of those three carries a hand-copied version of the shared path,
//! and each copy stopped at `clips.first()`. On a track split by an
//! interior cut they treated the head and left the tail alone — the same
//! defect the shared path had, surviving in three places that didn't get
//! the fix because they never called it.

use std::path::{Path, PathBuf};

use hound::WavReader;
use serde_json::json;

#[path = "support/harness.rs"]
mod harness;
use harness::{write_exact, Session};

const SAMPLE_RATE: u32 = 8_000;
const SOURCE_FRAMES: usize = 8_000;
const TONE_HZ: f32 = 1_000.0;

fn write_tone_wav(dir: &Path) -> PathBuf {
    write_exact(&dir.join("in.wav"), SAMPLE_RATE, SOURCE_FRAMES, |n| {
        let t = n as f32 / SAMPLE_RATE as f32;
        // Quiet enough that a +12 dB boost can't clip against full scale.
        (2.0 * std::f32::consts::PI * TONE_HZ * t).sin() * 0.1
    })
}

fn rms(samples: &[i16]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum: f64 = samples
        .iter()
        .map(|s| {
            let v = *s as f64 / 32_768.0;
            v * v
        })
        .sum();
    (sum / samples.len() as f64).sqrt() as f32
}

/// Cut an interior range, optionally EQ the track, render, return samples.
fn cut_then_maybe_eq(apply_eq: bool) -> Vec<i16> {
    let mut s = Session::empty();
    let src = write_tone_wav(s.dir.path());
    let out = s.dir.path().join("out.wav");

    s.ok("load", json!({ "path": src.to_string_lossy() }));
    let cut = s.ok(
        "cut_range",
        json!({ "track": 0, "start_sample": 2000, "end_sample": 3000 }),
    );
    let mut node_id = cut["node_id"].as_str().unwrap().to_string();

    if apply_eq {
        let res = s.ok(
            "eq",
            json!({
                "track": 0,
                "bands": [{ "freq_hz": TONE_HZ, "gain_db": 12.0, "q": 1.0 }]
            }),
        );
        node_id = res["node_id"].as_str().unwrap().to_string();
    }

    s.ok(
        "render_final",
        json!({
            "node_id": node_id,
            "format": "wav",
            "out_path": out.to_string_lossy(),
        }),
    );

    let mut reader = WavReader::open(&out).expect("open out");
    reader
        .samples::<i16>()
        .map(|r| r.expect("sample"))
        .collect()
}

/// A boost applied to a split track has to reach the second clip too.
#[test]
fn eq_covers_every_clip_of_a_split_track() {
    let plain = cut_then_maybe_eq(false);
    let boosted = cut_then_maybe_eq(true);

    assert_eq!(plain.len(), 7_000, "an interior cut leaves 7000 frames");
    assert_eq!(boosted.len(), plain.len(), "EQ must not change the length");

    // The seam: frames from 2000 on came from the second clip.
    let seam = 2_000;
    let head_gain = rms(&boosted[..seam]) / rms(&plain[..seam]);
    let tail_gain = rms(&boosted[seam..]) / rms(&plain[seam..]);

    assert!(
        head_gain > 2.0,
        "+12 dB should roughly quadruple the head's RMS, got {head_gain}x"
    );
    assert!(
        tail_gain > 2.0,
        "the tail is a second clip and was left untouched: {tail_gain}x \
         (head got {head_gain}x)"
    );
    assert!(
        (head_gain - tail_gain).abs() / head_gain < 0.1,
        "both halves should be boosted by the same amount, \
         got head {head_gain}x vs tail {tail_gain}x"
    );
}
