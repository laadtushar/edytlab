use crate::biquad::{biquad_process, BiquadCoeffs};
pub fn apply_high_pass(
    samples: &mut [f32],
    sr: u32,
    channels: usize,
    cutoff_hz: f32,
    start_sec: Option<f64>,
    end_sec: Option<f64>,
) {
    let channels = channels.max(1);
    let len_frames = samples.len() / channels;
    let start = start_sec
        .map(|s| ((s * sr as f64) as usize).min(len_frames))
        .unwrap_or(0);
    let end = end_sec
        .map(|e| ((e * sr as f64) as usize).min(len_frames))
        .unwrap_or(len_frames);
    let coeffs = BiquadCoeffs::high_pass(cutoff_hz, sr);
    biquad_process(samples, channels, &coeffs, start, end);
}

#[cfg(test)]
mod tests {
    use super::apply_high_pass;
    #[test]
    fn attenuates_dc() {
        let mut samples = vec![1.0f32; 4410];
        apply_high_pass(&mut samples, 44100, 1, 1000.0, None, None);
        let tail_mean: f32 = samples[4000..].iter().sum::<f32>() / 410.0;
        assert!(
            tail_mean.abs() < 0.01,
            "DC should be attenuated, got {tail_mean}"
        );
    }
}
