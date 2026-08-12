pub fn apply_tremolo(samples: &mut [f32], sr: u32, channels: usize, rate_hz: f32, depth: f32) {
    let channels = channels.max(1);
    let depth = depth.clamp(0.0, 1.0);
    let n_frames = samples.len() / channels;
    for frame in 0..n_frames {
        let lfo = (2.0 * std::f32::consts::PI * rate_hz * frame as f32 / sr as f32).cos();
        let gain = 1.0 - depth * (1.0 - lfo) / 2.0;
        for ch in 0..channels {
            samples[frame * channels + ch] *= gain;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::apply_tremolo;

    #[test]
    fn modulates_amplitude() {
        let mut samples = vec![1.0f32; 44100];
        apply_tremolo(&mut samples, 44100, 1, 5.0, 0.5);
        let at_max = samples[0];
        let at_min = samples[44100 / (5 * 2)];
        assert!(
            at_max > at_min,
            "tremolo should create amplitude variation, max={at_max} min={at_min}"
        );
    }
}
