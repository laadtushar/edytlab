use crate::biquad::{step, BiquadCoeffs, BiquadState};

/// Sweep a unity-gain band-pass up to two octaves either side of the base.
pub fn apply_wah_wah(
    samples: &mut [f32],
    sr: u32,
    channels: usize,
    rate_hz: f32,
    depth: f32,
    base_freq_hz: f32,
    resonance: f32,
) {
    let channels = channels.max(1);
    let sr = sr.max(1);
    let rate = if rate_hz.is_finite() {
        rate_hz.clamp(0.1, 10.0)
    } else {
        1.0
    };
    let depth = if depth.is_finite() {
        depth.clamp(0.0, 1.0)
    } else {
        0.5
    };
    let base = if base_freq_hz.is_finite() {
        base_freq_hz.max(1.0)
    } else {
        1000.0
    };
    let mut states = vec![BiquadState::new(); channels];
    let mut coeffs = BiquadCoeffs::band_pass(base, resonance, sr);
    for (frame, chunk) in samples.chunks_exact_mut(channels).enumerate() {
        if frame % 16 == 0 {
            let phase = 2.0 * std::f64::consts::PI * rate as f64 * frame as f64 / sr as f64;
            let center = base * 2.0_f32.powf(2.0 * depth * phase.cos() as f32);
            coeffs = BiquadCoeffs::band_pass(center, resonance, sr);
        }
        for (sample, state) in chunk.iter_mut().zip(&mut states) {
            *sample = step(&coeffs, state, *sample);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::PI;

    #[test]
    fn sweep_passes_a_tone_at_the_top_and_attenuates_it_half_a_period_later() {
        let sr = 48_000;
        let mut samples: Vec<_> = (0..sr * 2)
            .map(|n| (2.0 * PI * 4000.0 * n as f32 / sr as f32).sin())
            .collect();
        apply_wah_wah(&mut samples, sr, 1, 1.0, 1.0, 1000.0, 2.0);
        let rms = |start: usize| {
            (samples[start..start + 2400]
                .iter()
                .map(|v| v * v)
                .sum::<f32>()
                / 2400.0)
                .sqrt()
        };
        assert!(rms(47_000) > 0.5);
        assert!(rms(23_000) < 0.1);
    }

    #[test]
    fn silence_and_interleaved_stereo_are_independent() {
        let mut silence = vec![0.0; 9600];
        apply_wah_wah(&mut silence, 48_000, 2, 2.0, 1.0, 800.0, 2.0);
        assert!(silence.iter().all(|&x| x == 0.0));
        let mut stereo: Vec<_> = (0..4800)
            .flat_map(|n| [(2.0 * PI * 800.0 * n as f32 / 48_000.0).sin(), 0.0])
            .collect();
        let mut mono: Vec<_> = stereo.iter().step_by(2).copied().collect();
        apply_wah_wah(&mut stereo, 48_000, 2, 2.0, 0.5, 800.0, 2.0);
        apply_wah_wah(&mut mono, 48_000, 1, 2.0, 0.5, 800.0, 2.0);
        assert!(stereo
            .chunks_exact(2)
            .zip(mono)
            .all(|(frame, m)| frame[0] == m && frame[1] == 0.0));
    }
}
