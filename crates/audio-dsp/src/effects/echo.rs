pub fn apply_echo(samples: &mut Vec<f32>, sr: u32, channels: usize, delay_ms: f32, decay: f32) {
    let channels = channels.max(1);
    let delay_frames = ((delay_ms * 0.001 * sr as f32) as usize).max(1);
    let delay_samples = delay_frames * channels;
    let n = samples.len();
    let tail = delay_samples;
    samples.resize(n + tail, 0.0);
    for i in 0..n {
        let echo_idx = i + delay_samples;
        if echo_idx < samples.len() {
            samples[echo_idx] += samples[i] * decay;
        }
    }
}

/// Most feedback repeats the tail is extended for.
const MAX_REPEATS: usize = 16;
/// A repeat quieter than this (-60 dB) is not worth a longer tail.
const TAIL_FLOOR: f32 = 0.001;

/// Multi-tap delay: `y[n] = x[n] + sum_k gain_k * x[n - t_k] + feedback * y[n - T]`,
/// where `T` is the longest tap. With `feedback = 0` the impulse response is a
/// unit impulse plus one impulse of height `gain_k` at each `t_k`, exactly.
/// The buffer grows to hold the tail: `T` frames, or, with feedback, `T` for
/// each repeat until one falls below -60 dB (at most 16).
///
/// `taps` is `(delay_ms, gain)`; a tap rounds to the nearest frame (at least 1).
pub fn apply_multitap(
    samples: &mut Vec<f32>,
    sr: u32,
    channels: usize,
    taps: &[(f32, f32)],
    feedback: f32,
) {
    let channels = channels.max(1);
    if taps.is_empty() || samples.is_empty() {
        return;
    }
    let frames_of = |ms: f32| ((ms * 0.001 * sr as f32).round() as usize).max(1);
    let offsets: Vec<(usize, f32)> = taps
        .iter()
        .map(|&(ms, g)| (frames_of(ms) * channels, g))
        .collect();
    let longest = offsets.iter().map(|&(o, _)| o).max().unwrap_or(0);
    let fb = feedback.clamp(0.0, 0.95);
    let repeats = if fb > 0.0 {
        ((TAIL_FLOOR.ln() / fb.ln()).ceil() as usize).clamp(1, MAX_REPEATS)
    } else {
        0
    };
    let n = samples.len();
    let mut out = vec![0.0f32; n + longest * (1 + repeats)];
    for i in 0..out.len() {
        let mut y = if i < n { samples[i] } else { 0.0 };
        for &(o, g) in &offsets {
            if i >= o && i - o < n {
                y += g * samples[i - o];
            }
        }
        if fb > 0.0 && i >= longest {
            y += fb * out[i - longest];
        }
        out[i] = y;
    }
    *samples = out;
}

#[cfg(test)]
mod tests {
    use super::apply_echo;

    #[test]
    fn echo_appears_after_delay() {
        let mut samples = vec![0.0f32; 4410];
        samples[0] = 1.0;
        apply_echo(&mut samples, 44100, 1, 50.0, 0.5);
        let delay_frames = (50.0f32 * 0.001 * 44100.0) as usize;
        assert!(
            samples[delay_frames].abs() > 0.3,
            "echo peak expected at delay offset"
        );
    }

    use super::apply_multitap;

    fn impulse(frames: usize, channels: usize) -> Vec<f32> {
        let mut v = vec![0.0f32; frames * channels];
        for x in v.iter_mut().take(channels) {
            *x = 1.0;
        }
        v
    }

    #[test]
    fn multitap_impulse_response_is_exact() {
        // 1 kHz: 1 ms is one frame, so tap times land on whole frames.
        let mut v = impulse(10, 1);
        apply_multitap(
            &mut v,
            1000,
            1,
            &[(2.0, 0.5), (5.0, -0.25), (7.0, 0.125)],
            0.0,
        );
        assert_eq!(v.len(), 10 + 7);
        let mut want = vec![0.0f32; 17];
        want[0] = 1.0;
        want[2] = 0.5;
        want[5] = -0.25;
        want[7] = 0.125;
        assert_eq!(v, want);
    }

    #[test]
    fn multitap_feedback_repeats_the_pattern_at_the_longest_tap() {
        let mut v = impulse(4, 1);
        apply_multitap(&mut v, 1000, 1, &[(2.0, 0.5), (3.0, 0.25)], 0.5);
        // Pattern at 0, 2, 3; it recurs every 3 frames scaled by 0.5.
        assert_eq!(v[0], 1.0);
        assert_eq!(v[2], 0.5);
        assert_eq!(v[3], 0.25 + 0.5 * 1.0);
        assert_eq!(v[5], 0.5 * 0.5);
        assert_eq!(v[6], 0.5 * (0.25 + 0.5));
    }

    #[test]
    fn multitap_keeps_channels_apart() {
        let mut v = vec![0.0f32; 16];
        v[0] = 1.0; // left impulse only
        apply_multitap(&mut v, 1000, 2, &[(3.0, 0.5)], 0.0);
        assert_eq!(v[6], 0.5);
        assert_eq!(v[7], 0.0);
    }
}
