pub fn apply_stereo_widener(samples: &mut [f32], _sr: u32, channels: usize, width: f32) {
    if channels < 2 {
        return;
    }
    let n_frames = samples.len() / channels;
    for frame in 0..n_frames {
        let l = samples[frame * channels];
        let r = samples[frame * channels + 1];
        let mid = (l + r) / 2.0;
        let side = (l - r) / 2.0 * width;
        samples[frame * channels] = mid + side;
        samples[frame * channels + 1] = mid - side;
    }
}

#[cfg(test)]
mod tests {
    use super::apply_stereo_widener;

    #[test]
    fn width_zero_is_mono() {
        let mut samples = vec![0.8f32, 0.2, 0.6, 0.4];
        apply_stereo_widener(&mut samples, 44100, 2, 0.0);
        assert!(
            (samples[0] - samples[1]).abs() < 1e-5,
            "width=0 should give L==R"
        );
    }

    #[test]
    fn width_one_retains_stereo() {
        let mut samples = vec![0.8f32, 0.2, 0.6, 0.4];
        apply_stereo_widener(&mut samples, 44100, 2, 1.0);
        assert!(
            (samples[0] - samples[1]).abs() > 0.1,
            "stereo field preserved"
        );
    }
}
