use tools::{tool::reverse, Range};

#[test]
fn reverse_full_track() {
    let mut samples = vec![1.0f32, 2.0, 3.0, 4.0];
    reverse::apply_reverse(&mut samples, 4, None);
    assert_eq!(samples, vec![4.0, 3.0, 2.0, 1.0]);
}

#[test]
fn reverse_subrange_only() {
    // sr = 1, range [2.0, 6.0) → reverses indices 2..6, leaves 0..2 and 6..8 alone.
    let mut samples = vec![1.0f32, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0];
    reverse::apply_reverse(
        &mut samples,
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
        Some(Range {
            start_sec: 1.0,
            end_sec: 100.0,
        }),
    );
    assert_eq!(samples, vec![1.0, 4.0, 3.0, 2.0]);
}
