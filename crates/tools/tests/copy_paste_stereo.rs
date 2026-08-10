//! Copy/paste must be lossless on stereo.
//!
//! Both tools converted seconds to a *sample* index with no interleave
//! stride, so a copy grabbed half the requested duration and could
//! begin mid-frame; pasting that back swapped left and right, and an
//! odd-length clipboard swapped them for the whole remainder of the
//! track.

use tools::tool::{copy_region, paste_region};
use tools::Range;

/// Four stereo frames: left counts up, right is its negative, so any
/// channel swap is unambiguous.
fn stereo_ramp() -> Vec<f32> {
    vec![1.0, -1.0, 2.0, -2.0, 3.0, -3.0, 4.0, -4.0]
}

#[test]
fn copying_stereo_captures_whole_frames_of_the_requested_span() {
    let samples = stereo_ramp();
    let mut clipboard = None;
    // sr = 1, frames 1..3 → the second and third frames.
    copy_region::apply(
        &samples,
        1,
        2,
        Range {
            start_sec: 1.0,
            end_sec: 3.0,
        },
        &mut clipboard,
    )
    .expect("copy");

    assert_eq!(
        clipboard.expect("clipboard"),
        vec![2.0, -2.0, 3.0, -3.0],
        "two whole frames, channels paired"
    );
}

#[test]
fn a_stereo_copy_pasted_back_reproduces_the_original() {
    let samples = stereo_ramp();
    let mut clipboard = None;
    copy_region::apply(
        &samples,
        1,
        2,
        Range {
            start_sec: 0.0,
            end_sec: 2.0,
        },
        &mut clipboard,
    )
    .expect("copy");

    // Paste the first two frames back at the very start: the result
    // must be those frames twice, with left still on the left.
    let mut target = samples.clone();
    paste_region::apply(&mut target, 1, 2, 0.0, &clipboard).expect("paste");

    assert_eq!(
        target,
        vec![1.0, -1.0, 2.0, -2.0, 1.0, -1.0, 2.0, -2.0, 3.0, -3.0, 4.0, -4.0],
    );
    assert_eq!(target.len() % 2, 0, "buffer must stay frame-aligned");
}

/// Pasting at a position that lands mid-frame when counted in samples
/// must still splice on a frame boundary.
#[test]
fn pasting_stereo_splices_on_a_frame_boundary() {
    let mut target = stereo_ramp();
    let clipboard = Some(vec![9.0f32, -9.0]);
    paste_region::apply(&mut target, 1, 2, 1.0, &clipboard).expect("paste");
    assert_eq!(
        target,
        vec![1.0, -1.0, 9.0, -9.0, 2.0, -2.0, 3.0, -3.0, 4.0, -4.0],
        "inserted frame must not shift the interleaving"
    );
}
