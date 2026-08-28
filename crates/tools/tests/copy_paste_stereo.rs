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

    let clip = clipboard.expect("clipboard");
    assert_eq!(
        clip.samples,
        vec![2.0, -2.0, 3.0, -3.0],
        "two whole frames, channels paired"
    );
    // The capture format now travels with the samples (#239).
    assert_eq!((clip.sample_rate, clip.channels), (1, 2));
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
    let clipboard = Some(tools::Clipboard {
        samples: vec![9.0f32, -9.0],
        sample_rate: 1,
        channels: 2,
    });
    paste_region::apply(&mut target, 1, 2, 1.0, &clipboard).expect("paste");
    assert_eq!(
        target,
        vec![1.0, -1.0, 9.0, -9.0, 2.0, -2.0, 3.0, -3.0, 4.0, -4.0],
        "inserted frame must not shift the interleaving"
    );
}

// =============================================================================
// Cross-format pastes (#239)
// =============================================================================
//
// The clipboard used to be a bare `Vec<f32>` with no rate or channel
// count attached, so `paste_region` read it with the *destination's*
// stride. Copying two seconds of stereo into a mono track spliced four
// seconds of alternating left/right samples and returned `Ok` — the
// destination was corrupted, at the wrong length, with no warning.

fn clip(samples: Vec<f32>, sample_rate: u32, channels: u16) -> Option<tools::Clipboard> {
    Some(tools::Clipboard {
        samples,
        sample_rate,
        channels,
    })
}

/// Mono into stereo: the same signal in both channels, and — the part
/// that used to be wrong — the *same duration*.
#[test]
fn a_mono_copy_pasted_into_a_stereo_track_is_duplicated_across_channels() {
    let mut target = vec![1.0f32, -1.0, 2.0, -2.0];
    let cb = clip(vec![9.0, 8.0], 1, 1);
    paste_region::apply(&mut target, 1, 2, 0.0, &cb).expect("paste");

    assert_eq!(
        target,
        vec![9.0, 9.0, 8.0, 8.0, 1.0, -1.0, 2.0, -2.0],
        "each mono sample should become one stereo frame"
    );
    // Two mono frames in, two stereo frames out — not four.
    assert_eq!(target.len(), 8);
}

/// Stereo into mono: averaged, not de-interleaved as if it were mono.
///
/// Averaging rather than taking the left channel, because dropping a
/// channel silently loses anything panned to it.
#[test]
fn a_stereo_copy_pasted_into_a_mono_track_is_averaged() {
    let mut target = vec![1.0f32, 2.0];
    let cb = clip(vec![4.0, 6.0, 10.0, 20.0], 1, 2);
    paste_region::apply(&mut target, 1, 1, 0.0, &cb).expect("paste");

    assert_eq!(
        target,
        vec![5.0, 15.0, 1.0, 2.0],
        "two stereo frames should become two mono frames, averaged"
    );
    // The regression this issue is named for: verbatim splicing would
    // have inserted four samples for a two-frame copy.
    assert_eq!(
        target.len(),
        4,
        "a 2-frame stereo copy must not become 4 mono frames"
    );
}

/// A rate mismatch is refused by name rather than spliced at the wrong
/// speed. Resampling here would be a second resampler in a second
/// place; `resample_track` already exists to make the two agree.
#[test]
fn a_cross_rate_paste_is_refused_and_names_both_rates() {
    let mut target = vec![0.0f32; 4];
    let cb = clip(vec![1.0, 2.0], 44_100, 1);
    let err = paste_region::apply(&mut target, 48_000, 1, 0.0, &cb).expect_err("must refuse");

    let msg = err.to_string();
    assert!(
        msg.contains("44100") && msg.contains("48000"),
        "the error should name both rates so the user knows what to fix, got: {msg}"
    );
    assert_eq!(
        target,
        vec![0.0; 4],
        "a refused paste must not modify the track"
    );
}

/// A conversion with no unambiguous answer is refused rather than
/// guessed at.
#[test]
fn an_ambiguous_channel_conversion_is_refused() {
    let mut target = vec![0.0f32; 4];
    let cb = clip(vec![1.0; 18], 1, 6);
    let err = paste_region::apply(&mut target, 1, 2, 0.0, &cb).expect_err("must refuse");
    let msg = err.to_string();
    assert!(
        msg.contains('6') && msg.contains('2'),
        "the error should name both channel counts, got: {msg}"
    );
    assert_eq!(
        target,
        vec![0.0; 4],
        "a refused paste must not modify the track"
    );
}
