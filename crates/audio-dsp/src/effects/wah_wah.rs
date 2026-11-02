use crate::biquad::{step, BiquadCoeffs, BiquadState};

/// Lowest `base_freq_hz` the wah accepts. Two octaves down from here is
/// 50 Hz, where an f32 band-pass is still well inside the unit circle.
pub const BASE_FREQ_MIN_HZ: f32 = 200.0;
/// Highest `base_freq_hz` the wah accepts.
pub const BASE_FREQ_MAX_HZ: f32 = 5000.0;
/// Floor of the swept centre, whatever the base and depth. Below about
/// 40 Hz the band-pass's poles sit so close to `z = 1` that retuning
/// them while the filter runs pumps energy in (#846's review measured
/// `inf` at 192 kHz with a 2 Hz base).
pub const CENTRE_FLOOR_HZ: f32 = 40.0;
/// The coefficients are recomputed every this many frames. At rate 10,
/// depth 1 and Q 10, retuning every 16 frames left the stepping 11.5 dB
/// under the signal; every 4 puts it 25 dB under.
const RETUNE_FRAMES: usize = 4;

/// `base_freq_hz` held to [`BASE_FREQ_MIN_HZ`, `BASE_FREQ_MAX_HZ`]: NaN is
/// the 1 kHz default, and an infinity is the end it points at, so neither
/// reaches the filter or the node's label.
pub fn clamp_base_freq(base_freq_hz: f32) -> f32 {
    if base_freq_hz.is_nan() {
        1000.0
    } else {
        base_freq_hz.clamp(BASE_FREQ_MIN_HZ, BASE_FREQ_MAX_HZ)
    }
}

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
    let base = clamp_base_freq(base_freq_hz);
    let mut states = vec![BiquadState::new(); channels];
    let mut coeffs = BiquadCoeffs::band_pass(base, resonance, sr);
    for (frame, chunk) in samples.chunks_exact_mut(channels).enumerate() {
        if frame % RETUNE_FRAMES == 0 {
            let phase = 2.0 * std::f64::consts::PI * rate as f64 * frame as f64 / sr as f64;
            let center =
                (base * 2.0_f32.powf(2.0 * depth * phase.cos() as f32)).max(CENTRE_FLOOR_HZ);
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

    /// White noise, deterministic: xorshift32 mapped to [-1, 1).
    fn white_noise(len: usize) -> Vec<f32> {
        let mut x: u32 = 0x9E37_79B9;
        (0..len)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                (x as f32 / u32::MAX as f32) * 2.0 - 1.0
            })
            .collect()
    }

    #[test]
    fn the_lowest_base_at_192_khz_with_the_hardest_sweep_stays_bounded() {
        let sr = 192_000;
        let mut samples = white_noise(sr as usize * 2);
        apply_wah_wah(&mut samples, sr, 1, 10.0, 1.0, BASE_FREQ_MIN_HZ, 10.0);
        assert!(samples.iter().all(|s| s.is_finite()));
        let peak = samples.iter().fold(0.0_f32, |m, s| m.max(s.abs()));
        assert!(peak < 2.0, "peak {peak}");
    }

    #[test]
    fn a_base_below_the_range_is_clamped_up_and_stays_bounded() {
        for sr in [44_100, 96_000, 192_000] {
            for base in [1.0, 2.0, 10.0, 0.0, -5.0] {
                let mut samples = white_noise(sr as usize);
                apply_wah_wah(&mut samples, sr, 1, 10.0, 1.0, base, 10.0);
                let peak = samples.iter().fold(0.0_f32, |m, s| m.max(s.abs()));
                assert!(
                    peak.is_finite() && peak < 2.0,
                    "sr {sr} base {base}: peak {peak}"
                );
            }
        }
    }

    #[test]
    fn a_non_finite_base_is_clamped() {
        assert_eq!(clamp_base_freq(f32::INFINITY), BASE_FREQ_MAX_HZ);
        assert_eq!(clamp_base_freq(f32::NEG_INFINITY), BASE_FREQ_MIN_HZ);
        assert_eq!(clamp_base_freq(f32::NAN), 1000.0);
        assert_eq!(clamp_base_freq(1.0), BASE_FREQ_MIN_HZ);
        assert_eq!(clamp_base_freq(1e9), BASE_FREQ_MAX_HZ);
        assert_eq!(clamp_base_freq(1000.0), 1000.0);
        for base in [f32::INFINITY, f32::NEG_INFINITY, f32::NAN] {
            let mut samples = white_noise(48_000);
            apply_wah_wah(&mut samples, 48_000, 1, 1.0, 0.5, base, 2.0);
            assert!(samples.iter().all(|s| s.is_finite()));
        }
    }
}
