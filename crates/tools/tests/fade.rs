use tools::{tool::fade, Range};

#[test]
fn fade_in_starts_at_zero() {
    let mut samples = vec![1.0f32; 48_000];
    fade::apply_fade(
        &mut samples,
        48_000,
        Range {
            start_sec: 0.0,
            end_sec: 1.0,
        },
        fade::Kind::In,
    );
    assert!(samples[0].abs() < 1e-3);
    assert!((samples[47_999] - 1.0).abs() < 1e-3);
}

#[test]
fn fade_out_ends_at_zero() {
    let mut samples = vec![1.0f32; 48_000];
    fade::apply_fade(
        &mut samples,
        48_000,
        Range {
            start_sec: 0.0,
            end_sec: 1.0,
        },
        fade::Kind::Out,
    );
    assert!((samples[0] - 1.0).abs() < 1e-3);
    assert!(samples[47_999].abs() < 1e-3);
}

#[test]
fn samples_outside_range_unchanged() {
    let mut samples = vec![1.0f32; 96_000];
    fade::apply_fade(
        &mut samples,
        48_000,
        Range {
            start_sec: 0.0,
            end_sec: 1.0,
        },
        fade::Kind::In,
    );
    assert!((samples[80_000] - 1.0).abs() < 1e-9);
}
