pub fn apply_noise_gate(
    samples: &mut [f32],
    sr: u32,
    channels: usize,
    threshold_db: f32,
    attack_ms: f32,
    release_ms: f32,
) {
    let channels = channels.max(1);
    let threshold_lin = 10.0f32.powf(threshold_db / 20.0);
    let attack_coeff = (-1.0 / (attack_ms * 0.001 * sr as f32)).exp();
    let release_coeff = (-1.0 / (release_ms * 0.001 * sr as f32)).exp();
    let mut gain = 0.0f32;
    let n_frames = samples.len() / channels;
    for frame in 0..n_frames {
        let peak = (0..channels)
            .map(|ch| samples[frame * channels + ch].abs())
            .fold(0.0f32, f32::max);
        let target = if peak >= threshold_lin {
            1.0f32
        } else {
            0.0f32
        };
        let coeff = if target > gain {
            attack_coeff
        } else {
            release_coeff
        };
        gain = target + coeff * (gain - target);
        for ch in 0..channels {
            samples[frame * channels + ch] *= gain;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::apply_noise_gate;

    #[test]
    fn silences_below_threshold() {
        let mut samples: Vec<f32> = vec![0.005, 0.005, 0.5, 0.5, 0.005, 0.005];
        apply_noise_gate(&mut samples, 100, 1, -40.0, 1.0, 10.0);
        // Gate opens with attack, so gain ramps up but is close to 1.0
        assert!(samples[2] > 0.49, "above-threshold sample mostly untouched");
        assert!(samples[3] > 0.49, "above-threshold sample mostly untouched");
        // Gate closes with release, so below-threshold samples get silenced
        assert!(samples[0].abs() < 0.01, "below threshold silenced");
    }
}
