use crate::biquad::{biquad_process, BiquadCoeffs};
pub fn apply_de_esser(
    samples: &mut [f32],
    sr: u32,
    channels: usize,
    frequency_hz: f32,
    threshold_db: f32,
) {
    let channels = channels.max(1);
    let threshold_lin = 10.0f32.powf(threshold_db / 20.0);
    let n_frames = samples.len() / channels;
    let coeffs = BiquadCoeffs::high_pass(frequency_hz, sr);
    let mut detector: Vec<f32> = samples.to_vec();
    biquad_process(&mut detector, channels, &coeffs, 0, n_frames);
    let attack_coeff = (-1.0f32 / (2.0 * 0.001 * sr as f32)).exp();
    let release_coeff = (-1.0f32 / (100.0 * 0.001 * sr as f32)).exp();
    let mut env = 0.0f32;
    for frame in 0..n_frames {
        let peak = (0..channels)
            .map(|ch| detector[frame * channels + ch].abs())
            .fold(0.0f32, f32::max);
        let coeff = if peak > env {
            attack_coeff
        } else {
            release_coeff
        };
        env = peak + coeff * (env - peak);
        if env > threshold_lin {
            let reduction = threshold_lin / env;
            for ch in 0..channels {
                samples[frame * channels + ch] *= reduction;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::apply_de_esser;

    #[test]
    fn reduces_high_freq_energy() {
        let mut samples: Vec<f32> = (0..44100).map(|i| ((i as f32 * 0.1).sin() * 0.9)).collect();
        let before_max = samples.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        apply_de_esser(&mut samples, 44100, 1, 8000.0, -20.0);
        let after_max = samples.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        assert!(
            after_max <= before_max + 1e-4,
            "de-esser should not amplify"
        );
    }
}
