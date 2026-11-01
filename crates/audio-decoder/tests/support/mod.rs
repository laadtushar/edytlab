//! What the decoder's fixture tests share: where the fixtures are, one
//! channel of a decode, and the strongest tone in a signal.
//!
//! A test binary includes this with `mod support;` and uses a subset.
#![allow(dead_code)]

use std::path::{Path, PathBuf};

use audio_decoder::DecodedAudio;

/// The fixture `name` in `tests/fixtures/`.
pub fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

/// The samples of one channel.
pub fn channel(d: &DecodedAudio, c: usize) -> Vec<f32> {
    d.samples
        .iter()
        .skip(c)
        .step_by(d.channels as usize)
        .copied()
        .collect()
}

/// The frequency of the strongest component of `x`: a Hann-windowed FFT over
/// the middle of the signal (the ends hold codec padding), with the peak
/// refined by a parabola through the log magnitudes of its three bins.
/// `x` must hold more than 16,384 samples.
pub fn peak_hz(x: &[f32], rate: u32) -> f32 {
    use rustfft::num_complex::Complex;
    let n = 16_384usize;
    let start = (x.len() - n) / 2;
    let mut buf: Vec<Complex<f32>> = x[start..start + n]
        .iter()
        .enumerate()
        .map(|(i, &s)| {
            let w = 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / n as f32).cos();
            Complex::new(s * w, 0.0)
        })
        .collect();
    rustfft::FftPlanner::<f32>::new()
        .plan_fft_forward(n)
        .process(&mut buf);
    let mag: Vec<f32> = buf[..n / 2].iter().map(|c| c.norm()).collect();
    let (k, _) = mag
        .iter()
        .enumerate()
        .skip(1)
        .fold((1, 0.0f32), |a, (i, &m)| if m > a.1 { (i, m) } else { a });
    let (a, b, c) = (
        mag[k - 1].max(1e-12).ln(),
        mag[k].max(1e-12).ln(),
        mag[k + 1].max(1e-12).ln(),
    );
    let delta = 0.5 * (a - c) / (a - 2.0 * b + c);
    (k as f32 + delta) * rate as f32 / n as f32
}

/// The largest magnitude in `x`.
pub fn peak(x: &[f32]) -> f32 {
    x.iter().fold(0.0, |m, s| m.max(s.abs()))
}
