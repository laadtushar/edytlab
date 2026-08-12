pub fn apply_leveler(
    samples: &mut [f32],
    sr: u32,
    channels: usize,
    target_db: f32,
    window_ms: u32,
) {
    let channels = channels.max(1);
    let target_rms = 10.0f32.powf(target_db / 20.0);
    let window_frames = ((window_ms as f32 * 0.001 * sr as f32) as usize).max(1);
    let n_frames = samples.len() / channels;
    let mut frame = 0;
    while frame < n_frames {
        let end = (frame + window_frames).min(n_frames);
        let rms: f32 = {
            let slice_start = frame * channels;
            let slice_end = end * channels;
            let sum_sq: f32 = samples[slice_start..slice_end].iter().map(|s| s * s).sum();
            (sum_sq / (slice_end - slice_start) as f32).sqrt()
        };
        if rms > 1e-6 {
            let gain = (target_rms / rms).min(10.0);
            for s in &mut samples[frame * channels..end * channels] {
                *s *= gain;
            }
        }
        frame = end;
    }
}

#[cfg(test)]
mod tests {
    use super::apply_leveler;

    #[test]
    fn boosts_quiet_section() {
        let mut samples: Vec<f32> = (0..200)
            .map(|i| if i < 100 { 0.1f32 } else { 0.9 })
            .collect();
        apply_leveler(&mut samples, 44100, 1, -12.0, 1);
        let quiet_avg: f32 = samples[..100].iter().map(|s| s.abs()).sum::<f32>() / 100.0;
        assert!(quiet_avg > 0.15, "quiet section boosted, got {quiet_avg}");
    }
}
