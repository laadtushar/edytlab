use tools::{tool::reverse, Range};

#[test]
fn reverse_full_track() {
    let mut samples = vec![1.0f32, 2.0, 3.0, 4.0];
    reverse::apply_reverse(&mut samples, 4, 1, None);
    assert_eq!(samples, vec![4.0, 3.0, 2.0, 1.0]);
}

#[test]
fn reverse_subrange_only() {
    // sr = 1, range [2.0, 6.0) → reverses indices 2..6, leaves 0..2 and 6..8 alone.
    let mut samples = vec![1.0f32, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0];
    reverse::apply_reverse(
        &mut samples,
        1,
        1,
        Some(Range {
            start_sec: 2.0,
            end_sec: 6.0,
        }),
    );
    assert_eq!(samples, vec![1.0, 2.0, 6.0, 5.0, 4.0, 3.0, 7.0, 8.0]);
}

#[test]
fn reverse_empty_range_is_noop() {
    let mut samples = vec![1.0f32, 2.0, 3.0];
    reverse::apply_reverse(
        &mut samples,
        1,
        1,
        Some(Range {
            start_sec: 1.0,
            end_sec: 1.0,
        }),
    );
    assert_eq!(samples, vec![1.0, 2.0, 3.0]);
}

#[test]
fn reverse_clamps_end_to_buffer_length() {
    // Range past end of buffer should clamp, not panic.
    let mut samples = vec![1.0f32, 2.0, 3.0, 4.0];
    reverse::apply_reverse(
        &mut samples,
        1,
        1,
        Some(Range {
            start_sec: 1.0,
            end_sec: 100.0,
        }),
    );
    assert_eq!(samples, vec![1.0, 4.0, 3.0, 2.0]);
}

/// Reversing stereo must reverse the *frames*, keeping left on the left.
///
/// `samples.reverse()` on an interleaved buffer turns [L0,R0,L1,R1]
/// into [R1,L1,R0,L0]: the frames come back in the right order, but
/// every frame has its channels swapped, mirroring the stereo image.
#[test]
fn reverse_stereo_preserves_channel_order() {
    // Left channel counts up 1,2,3,4; right is the negative of it, so a
    // swap is unambiguous in the assertion.
    let mut samples = vec![1.0f32, -1.0, 2.0, -2.0, 3.0, -3.0, 4.0, -4.0];
    reverse::apply_reverse(&mut samples, 1, 2, None);
    assert_eq!(
        samples,
        vec![4.0, -4.0, 3.0, -3.0, 2.0, -2.0, 1.0, -1.0],
        "frames must reverse with left still on the left"
    );
}

/// A stereo sub-range is expressed in seconds, so it must convert
/// through frames — not sample indices, which would cover half the span.
#[test]
fn reverse_stereo_subrange_uses_frames_not_samples() {
    // sr = 1, 4 stereo frames. Range [1.0, 3.0) covers frames 1 and 2.
    let mut samples = vec![1.0f32, -1.0, 2.0, -2.0, 3.0, -3.0, 4.0, -4.0];
    reverse::apply_reverse(
        &mut samples,
        1,
        2,
        Some(Range {
            start_sec: 1.0,
            end_sec: 3.0,
        }),
    );
    assert_eq!(
        samples,
        vec![1.0, -1.0, 3.0, -3.0, 2.0, -2.0, 4.0, -4.0],
        "only frames 1..3 should move, and channels must stay paired"
    );
}
