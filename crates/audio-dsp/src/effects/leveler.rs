//! RMS levelling with a continuous gain curve.
//!
//! The gain is measured per window and *applied* per sample, ramped
//! between window centres. It used to be applied per window as a
//! rectangle: every 100 ms boundary was a step change in gain, and on
//! the material this tool exists for — audio whose level actually
//! varies — that is a click. Measured on a 997 Hz tone ramping
//! 0.05 → 0.5, one boundary stepped 0.140 (about -17 dBFS) where the
//! largest step anywhere in the *input* was 0.071.
//!
//! A 1 kHz probe hides it completely, because at 44.1 kHz every 100 ms
//! boundary lands on an exact zero crossing. 997 Hz is the standard
//! choice for exactly that reason, and it is what the test uses.

/// Ceiling on the boost, as a linear gain (+12 dB).
///
/// It was 10.0 — +20 dB. Two windows either side of a quiet-to-loud
/// transition could sit at 10.0 and 0.39, a 28 dB swing, and on a
/// near-silent window +20 dB is a noise-floor amplifier rather than a
/// leveller. Ramping the gain removes the click; the cap is what stops
/// the ramp being an audible swell of hiss.
const MAX_BOOST: f32 = 4.0;

/// Per-window gain, and the frame each window's centre sits at.
///
/// Split out so the choice of gain can be tested apart from how it is
/// applied.
fn window_gains(
    samples: &[f32],
    channels: usize,
    target_rms: f32,
    window_frames: usize,
) -> Vec<(usize, f32)> {
    let n_frames = samples.len() / channels;
    let mut out = Vec::new();
    let mut frame = 0;
    while frame < n_frames {
        let end = (frame + window_frames).min(n_frames);
        let slice = &samples[frame * channels..end * channels];
        let sum_sq: f32 = slice.iter().map(|s| s * s).sum();
        let rms = (sum_sq / slice.len() as f32).sqrt();
        // A window with nothing in it keeps unity. Carrying the
        // neighbouring gain across would boost the noise floor of a
        // silence, which is the one thing a leveller must not do.
        let gain = if rms > 1e-6 {
            (target_rms / rms).min(MAX_BOOST)
        } else {
            1.0
        };
        out.push(((frame + end) / 2, gain));
        frame = end;
    }
    out
}

/// Level an interleaved buffer in place towards `target_db` RMS.
///
/// `window_ms` sets how often the gain is *measured*. It is applied
/// continuously: constant before the first window centre and after the
/// last, linearly interpolated in between, so the gain curve has no
/// steps and neither does the audio.
pub fn apply_leveler(
    samples: &mut [f32],
    sr: u32,
    channels: usize,
    target_db: f32,
    window_ms: u32,
) {
    let channels = channels.max(1);
    if samples.len() < channels {
        return;
    }
    let target_rms = 10.0f32.powf(target_db / 20.0);
    let window_frames = ((window_ms as f32 * 0.001 * sr as f32) as usize).max(1);
    let n_frames = samples.len() / channels;

    let gains = window_gains(samples, channels, target_rms, window_frames);
    if gains.is_empty() {
        return;
    }

    // Walk the frames once, advancing through the window list rather
    // than searching it per frame.
    let mut next = 0;
    for f in 0..n_frames {
        while next + 1 < gains.len() && gains[next + 1].0 <= f {
            next += 1;
        }
        let gain = if f <= gains[0].0 {
            gains[0].1
        } else if next + 1 >= gains.len() {
            gains[next].1
        } else {
            let (a_frame, a_gain) = gains[next];
            let (b_frame, b_gain) = gains[next + 1];
            let span = (b_frame - a_frame) as f32;
            let t = if span > 0.0 {
                (f - a_frame) as f32 / span
            } else {
                0.0
            };
            a_gain + (b_gain - a_gain) * t
        };
        for c in 0..channels {
            samples[f * channels + c] *= gain;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{apply_leveler, window_gains, MAX_BOOST};

    #[test]
    fn boosts_quiet_section() {
        let mut samples: Vec<f32> = (0..200)
            .map(|i| if i < 100 { 0.1f32 } else { 0.9 })
            .collect();
        apply_leveler(&mut samples, 44100, 1, -12.0, 1);
        let quiet_avg: f32 = samples[..100].iter().map(|s| s.abs()).sum::<f32>() / 100.0;
        assert!(quiet_avg > 0.15, "quiet section boosted, got {quiet_avg}");
    }

    /// The probe from the report: 1 s of 997 Hz at 44.1 kHz, ramping
    /// 0.05 → 0.5.
    ///
    /// 997 rather than 1000 on purpose. At 44.1 kHz a 1 kHz tone puts
    /// every 100 ms window boundary on an exact zero crossing, so the
    /// rectangular gain multiplied ~0 by a different number and the
    /// artifact vanished. This probe is what makes it visible.
    fn ramping_tone() -> Vec<f32> {
        let sr = 44_100.0;
        (0..44_100)
            .map(|i| {
                let t = i as f32 / sr;
                let amp = 0.05 + (0.5 - 0.05) * t;
                amp * (2.0 * std::f32::consts::PI * 997.0 * t).sin()
            })
            .collect()
    }

    fn largest_step(xs: &[f32]) -> f32 {
        xs.windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0f32, f32::max)
    }

    #[test]
    fn adds_no_discontinuity_the_input_did_not_have() {
        let input = ramping_tone();
        let mut output = input.clone();
        apply_leveler(&mut output, 44_100, 1, -12.0, 100);

        let before = largest_step(&input);
        let after = largest_step(&output);

        // Levelling this probe pulls the loud end *down*, so a
        // continuous gain cannot introduce a step larger than the
        // input's own. The old rectangular gain produced 0.140 here
        // against an input maximum of 0.071.
        assert!(
            after <= before + 1e-4,
            "output has a step of {after} where the input's largest is {before} — \
             the gain is being applied as a staircase again"
        );
    }

    /// The gain curve itself, read back from a signal whose level
    /// jumps.
    ///
    /// Recovered as `output / input` pointwise, which needs an input
    /// that is never zero — hence the two DC levels rather than a tone.
    /// An earlier version of this test levelled a *constant* buffer and
    /// read that: every window then wants the same gain, so the curve
    /// was flat no matter how it was applied and the test passed
    /// against the rectangular gain it was written to catch. Reading
    /// the curve off the varying signal is the whole point.
    #[test]
    fn the_gain_curve_is_continuous_across_a_level_jump() {
        // A quiet half and a loud half: consecutive windows want very
        // different gains, which is the shape that produced the 28 dB
        // boundary.
        let mut input: Vec<f32> = Vec::new();
        input.extend(std::iter::repeat_n(0.02f32, 4_410 * 3));
        input.extend(std::iter::repeat_n(0.9f32, 4_410 * 3));

        let mut output = input.clone();
        apply_leveler(&mut output, 44_100, 1, -12.0, 100);

        let curve: Vec<f32> = output.iter().zip(&input).map(|(o, i)| o / i).collect();
        let jump = largest_step(&curve);

        // The gain moves across a window's span rather than at a point,
        // so no single frame can carry more than a sliver of it.
        assert!(
            jump < 0.01,
            "the gain curve steps by {jump} between adjacent frames"
        );
    }

    #[test]
    fn a_silent_window_is_left_alone_rather_than_amplified() {
        let gains = window_gains(&[0.0; 400], 1, 0.25, 100);
        assert!(!gains.is_empty());
        for (_, g) in gains {
            assert_eq!(g, 1.0, "silence was given gain");
        }
    }

    #[test]
    fn the_boost_is_capped() {
        // Far below the target, so the uncapped gain would be enormous.
        let gains = window_gains(&[0.0001; 400], 1, 0.25, 100);
        for (_, g) in gains {
            assert!(g <= MAX_BOOST, "gain {g} exceeds the cap");
        }
    }
}
