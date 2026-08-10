use tools::{tool::fade, Range};

#[test]
fn fade_in_starts_at_zero() {
    let mut samples = vec![1.0f32; 48_000];
    fade::apply_fade(
        &mut samples,
        48_000,
        1,
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
        1,
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
        1,
        Range {
            start_sec: 0.0,
            end_sec: 1.0,
        },
        fade::Kind::In,
    );
    assert!((samples[80_000] - 1.0).abs() < 1e-9);
}

/// A fade over stereo must cover the whole requested span and treat the
/// two channels identically.
///
/// Seconds used to convert straight to a *sample* index with no channel
/// stride, so on stereo the ramp finished halfway through the requested
/// window — leaving a full-scale step from silence back to full level
/// at the midpoint, which is an audible click.
#[test]
fn fade_out_over_stereo_covers_the_whole_range() {
    const SR: u32 = 48_000;
    // 2 s of stereo: 96_000 frames, 192_000 interleaved samples.
    let mut samples = vec![1.0f32; SR as usize * 2 * 2];
    fade::apply_fade(
        &mut samples,
        SR,
        2,
        Range {
            start_sec: 0.0,
            end_sec: 2.0,
        },
        fade::Kind::Out,
    );

    let frame = |f: usize| (samples[f * 2], samples[f * 2 + 1]);

    let (l0, r0) = frame(0);
    assert!((l0 - 1.0).abs() < 1e-3, "fade should start at full level");
    assert_eq!(l0, r0, "both channels must get the same gain");

    // Halfway through the *requested* range, not halfway through the
    // buffer-as-mono — this is the assertion the old code failed.
    let (lmid, rmid) = frame(SR as usize);
    assert!(
        (lmid - 0.5).abs() < 1e-2,
        "at 1s of a 2s fade the gain should be ~0.5, got {lmid}"
    );
    assert_eq!(lmid, rmid, "both channels must get the same gain");

    let (lend, rend) = frame(SR as usize * 2 - 1);
    assert!(lend.abs() < 1e-3, "fade should reach silence, got {lend}");
    assert_eq!(lend, rend, "both channels must get the same gain");
}
