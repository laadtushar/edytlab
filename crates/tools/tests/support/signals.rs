//! The synthetic voice, noise and mixes the speech-keyed tests listen to.
//!
//! Each is deterministic, so a run is repeatable.
#![allow(dead_code)]

/// Voiced sound: six harmonics of 140 Hz falling as 1/k, about -21 dBFS.
pub fn voice_burst(rate: u32, seconds: f64) -> Vec<f32> {
    (0..(rate as f64 * seconds) as usize)
        .map(|n| {
            let t = n as f64 / rate as f64;
            (1..=6)
                .map(|k| 0.1 / k as f64 * (std::f64::consts::TAU * k as f64 * 140.0 * t).sin())
                .sum::<f64>() as f32
        })
        .collect()
}

/// White noise at `rms`, from a fixed xorshift so the run is repeatable.
pub fn noise_burst(rate: u32, seconds: f64, rms: f32) -> Vec<f32> {
    let mut state = 0x2545_f491u32;
    (0..(rate as f64 * seconds) as usize)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            (state as f32 / u32::MAX as f32 * 2.0 - 1.0) * rms * 3f32.sqrt()
        })
        .collect()
}

/// `seconds` of silence at `rate` with each `(at_sec, signal)` mixed in.
pub fn track_with(rate: u32, seconds: f64, parts: &[(f64, Vec<f32>)]) -> Vec<f32> {
    let mut buf = vec![0.0f32; (rate as f64 * seconds) as usize];
    for (at, signal) in parts {
        let start = (at * rate as f64).round() as usize;
        for (d, s) in buf[start..].iter_mut().zip(signal) {
            *d += *s;
        }
    }
    buf
}
