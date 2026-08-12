use crate::biquad::{biquad_process, BiquadCoeffs};
pub fn apply_notch(
    samples: &mut [f32],
    sr: u32,
    channels: usize,
    center_hz: f32,
    q: f32,
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
    let coeffs = BiquadCoeffs::notch(center_hz, q, sr);
    biquad_process(samples, channels, &coeffs, start, end);
}

#[cfg(test)]
mod tests {
    use super::apply_notch;
    #[test]
    fn notch_does_not_crash() {
        let mut samples = vec![0.5f32; 4410];
        apply_notch(&mut samples, 44100, 1, 60.0, 1.0, None, None);
        assert_eq!(samples.len(), 4410);
    }
}
