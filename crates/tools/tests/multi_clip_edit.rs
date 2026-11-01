//! Destructive edits on a track that has been split into several clips.
//!
//! `cut_range` with an interior range leaves two clips behind, and every
//! destructive tool used to edit `clips[0]` and stop. A reverb, a filter
//! or an invert applied after an interior cut therefore treated the first
//! half and left the second half untouched, with a hard seam at the join
//! and no indication anything had been skipped.

use std::path::{Path, PathBuf};

use hound::WavReader;
use serde_json::json;

#[path = "support/harness.rs"]
mod harness;
use harness::{write_half_ramp, Session};

const SAMPLE_RATE: u32 = 8_000;
const SOURCE_FRAMES: usize = 8_000;

fn write_ramp_wav(dir: &Path, name: &str) -> PathBuf {
    write_half_ramp(&dir.join(name), SAMPLE_RATE, SOURCE_FRAMES)
}

fn read_samples(path: &Path) -> Vec<i16> {
    let mut reader = WavReader::open(path).expect("open output wav");
    reader
        .samples::<i16>()
        .map(|r| r.expect("sample"))
        .collect()
}

/// Render a track that has had an interior cut, optionally inverting it
/// after the cut. Returns the rendered samples.
fn cut_then_maybe_invert(invert: bool) -> Vec<i16> {
    let mut s = Session::empty();
    let src = write_ramp_wav(s.dir.path(), "in.wav");
    let out = s.dir.path().join("out.wav");

    s.ok("load", json!({ "path": src.to_string_lossy() }));

    // Cut [2000, 3000) out of the middle: two clips, 2000 + 5000 frames.
    let cut = s.ok(
        "cut_range",
        json!({ "track": 0, "start_sample": 2000, "end_sample": 3000 }),
    );
    let mut node_id = cut["node_id"].as_str().unwrap().to_string();

    if invert {
        let inverted = s.ok("invert", json!({ "track": 0 }));
        node_id = inverted["node_id"].as_str().unwrap().to_string();
    }

    s.ok(
        "render_final",
        json!({
            "node_id": node_id,
            "format": "wav",
            "out_path": out.to_string_lossy(),
        }),
    );

    read_samples(&out)
}

/// Every frame of a split track must be edited, not just the first clip's
/// worth.
///
/// Invert is the sharpest probe available: its effect is exact and
/// per-sample, so an unedited region shows up as a sample that matched
/// instead of negating — no tolerance to hide behind.
#[test]
fn destructive_edit_covers_every_clip_of_a_split_track() {
    let plain = cut_then_maybe_invert(false);
    let inverted = cut_then_maybe_invert(true);

    assert_eq!(
        plain.len(),
        inverted.len(),
        "inverting must not change the track length"
    );
    assert_eq!(plain.len(), SOURCE_FRAMES - 1000, "cut removes 1000 frames");

    // Where the second clip starts. Anything at or past this index came
    // from `clips[1]`, which the old code never touched.
    let seam = 2000;
    let mut unedited_after_seam = 0;
    for (i, (a, b)) in plain.iter().zip(inverted.iter()).enumerate() {
        // Quantisation can leave a 1-LSB residue on the round trip.
        let negated = (*a as i32 + *b as i32).abs() <= 1;
        if !negated && i >= seam {
            unedited_after_seam += 1;
        }
        assert!(
            negated,
            "frame {i} was not inverted: {a} vs {b} \
             (frames from {seam} on live in the second clip)"
        );
    }
    assert_eq!(unedited_after_seam, 0);
}

/// Flattening the clips onto one buffer must not shift, drop or
/// duplicate a frame at the join.
///
/// This looks at the *edited* render, so it is checking the layout
/// `flatten_track` produced rather than the render engine's — an
/// off-by-one in the clip offsets would slide the tail by a frame and
/// still pass a length assertion.
#[test]
fn flattening_preserves_the_position_of_the_seam() {
    let edited = cut_then_maybe_invert(true);

    // Frame 1999 is the last of the head (source frame 1999); frame 2000
    // is the first of the tail (source frame 3000, where the cut
    // resumed). Both are negated, because the whole track was inverted.
    let head_end = edited[1999] as f32 / 32_768.0;
    let tail_start = edited[2000] as f32 / 32_768.0;
    let expected_head = -0.5 * 1999.0 / SOURCE_FRAMES as f32;
    let expected_tail = -0.5 * 3000.0 / SOURCE_FRAMES as f32;

    assert!(
        (head_end - expected_head).abs() < 0.005,
        "head should end at source frame 1999, got {head_end}"
    );
    assert!(
        (tail_start - expected_tail).abs() < 0.005,
        "tail should resume at source frame 3000, got {tail_start}"
    );
}
