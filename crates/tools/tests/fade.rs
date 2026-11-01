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

#[test]
fn curves_share_a_half_open_frame_grid_and_gain_across_channels() {
    let cases = [
        (fade::Curve::Linear, [0.0, 0.25, 0.5, 0.75, 1.0]),
        (fade::Curve::Exponential, [0.0, 0.0625, 0.25, 0.5625, 1.0]),
        (
            fade::Curve::Logarithmic,
            [0.0, 0.5, std::f32::consts::FRAC_1_SQRT_2, 0.8660254, 1.0],
        ),
        (fade::Curve::SCurve, [0.0, 0.15625, 0.5, 0.84375, 1.0]),
    ];
    for channels in [1, 2] {
        for (curve, gains) in cases {
            for kind in [fade::Kind::In, fade::Kind::Out] {
                // Signed, unequal channel values expose interleave mistakes.
                let input: Vec<f32> = (0..6 * channels)
                    .map(|i| if i % 2 == 0 { 0.75 } else { -0.375 })
                    .collect();
                let mut actual = input.clone();
                fade::apply_fade_with_curve(
                    &mut actual,
                    4,
                    channels,
                    Range {
                        start_sec: 0.25,
                        end_sec: 1.25,
                    },
                    kind,
                    curve,
                );
                assert_eq!(&actual[..channels], &input[..channels]);
                assert_eq!(&actual[5 * channels..], &input[5 * channels..]);
                for frame in 1..5 {
                    let index = match kind {
                        fade::Kind::In => frame - 1,
                        fade::Kind::Out => 5 - frame,
                    };
                    for ch in 0..channels {
                        let sample = frame * channels + ch;
                        assert_eq!(
                            actual[sample],
                            input[sample] * gains[index],
                            "{curve:?} {kind:?} frame {frame} channel {ch}"
                        );
                    }
                }
            }
        }
    }
}

// Frozen pre-847 algorithm: comparing two current entry points alone would
// miss a shared change to the old arithmetic and content-addressed filenames.
fn legacy_fade(samples: &mut [f32], rate: u32, channels: usize, range: Range, kind: fade::Kind) {
    let stride = channels.max(1);
    let frames = samples.len() / stride;
    let start = ((range.start_sec * rate as f64) as usize).min(frames);
    let end = ((range.end_sec * rate as f64) as usize).min(frames);
    if end <= start {
        return;
    }
    let len = (end - start) as f32;
    for frame in start..end {
        let t = (frame - start) as f32 / len;
        let gain = match kind {
            fade::Kind::In => t,
            fade::Kind::Out => 1.0 - t,
        };
        for ch in 0..stride {
            samples[frame * stride + ch] *= gain;
        }
    }
}

#[test]
fn linear_entry_points_preserve_legacy_float_bits_and_clamping() {
    for channels in [0, 1, 2, 3] {
        for count in [0, 1, 2, 3, 17, 38] {
            for (start_sec, end_sec) in [
                (0.0, 1.0),
                (0.25, 1.125),
                (0.0, 20.0),
                (20.0, 21.0),
                (0.5, 0.5),
                (1.0, 0.25),
                (-1.0, 0.5),
            ] {
                for (kind, spelling) in [(fade::Kind::In, "in"), (fade::Kind::Out, "out")] {
                    let input: Vec<f32> = (0..count).map(|i| (i as f32 - 8.125) / 13.75).collect();
                    let range = Range { start_sec, end_sec };
                    let mut expected = input.clone();
                    legacy_fade(&mut expected, 8, channels, range, kind);
                    let expected: Vec<u32> = expected.iter().map(|s| s.to_bits()).collect();
                    for explicit in [false, true] {
                        let mut actual = input.clone();
                        if explicit {
                            fade::apply_fade_with_curve(
                                &mut actual,
                                8,
                                channels,
                                range,
                                kind,
                                fade::Curve::Linear,
                            );
                        } else {
                            fade::apply_fade(&mut actual, 8, channels, range, kind);
                        }
                        assert_eq!(
                            actual.iter().map(|s| s.to_bits()).collect::<Vec<_>>(),
                            expected
                        );
                    }
                    // The range resolver intentionally rejects degenerate ranges;
                    // characterize their DSP behavior above without changing it.
                    if start_sec >= 0.0 && end_sec > start_sec {
                        for curve in [None, Some("linear")] {
                            let mut args = serde_json::json!({"kind": spelling, "range": range});
                            if let Some(curve) = curve {
                                args["curve"] = curve.into();
                            }
                            let params: fade::FadeParams = serde_json::from_value(args).unwrap();
                            let mut actual = input.clone();
                            fade::dispatch_fade(params, "", &mut actual, 8, channels).unwrap();
                            assert_eq!(
                                actual.iter().map(|s| s.to_bits()).collect::<Vec<_>>(),
                                expected
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn shaped_dispatch_and_short_buffers_keep_frame_boundaries() {
    let params: fade::FadeParams = serde_json::from_value(serde_json::json!({
        "curve": "exponential", "range": {"start_sec": 0.0, "end_sec": 1.0}
    }))
    .unwrap();
    let mut samples = [0.5, -0.25, 0.5, -0.25, 0.75];
    fade::dispatch_fade(params, "", &mut samples, 2, 2).unwrap();
    // Omitted kind is Out; trailing incomplete frame is not edited.
    assert_eq!(samples, [0.5, -0.25, 0.125, -0.0625, 0.75]);
    for curve in [
        fade::Curve::Exponential,
        fade::Curve::Logarithmic,
        fade::Curve::SCurve,
    ] {
        for kind in [fade::Kind::In, fade::Kind::Out] {
            for channels in [0, 1, 2] {
                for input in [vec![], vec![0.5], vec![0.5, -0.25, 0.75]] {
                    for (start_sec, end_sec) in [(0.0, 20.0), (20.0, 21.0), (0.5, 0.5), (1.0, 0.0)]
                    {
                        let mut actual = input.clone();
                        fade::apply_fade_with_curve(
                            &mut actual,
                            1,
                            channels,
                            Range { start_sec, end_sec },
                            kind,
                            curve,
                        );
                        assert_eq!(actual.len(), input.len());
                        if start_sec >= 20.0 || end_sec <= start_sec {
                            assert_eq!(actual, input);
                        } else {
                            let stride = channels.max(1);
                            let complete = input.len() / stride * stride;
                            assert_eq!(&actual[complete..], &input[complete..]);
                            if complete > 0 {
                                let first_gain = match kind {
                                    fade::Kind::In => 0.0,
                                    fade::Kind::Out => 1.0,
                                };
                                for ch in 0..stride {
                                    assert_eq!(actual[ch], input[ch] * first_gain);
                                }
                            }
                            for (&got, &original) in
                                actual[..complete].iter().zip(&input[..complete])
                            {
                                assert!(got.is_finite() && got.abs() <= original.abs());
                            }
                        }
                    }
                }
            }
        }
    }
}
