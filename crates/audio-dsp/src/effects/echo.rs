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
}
