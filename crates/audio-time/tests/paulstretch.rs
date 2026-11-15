//! Contract tests for `paulstretch`, the extreme time stretch.
//!
//! Each asserts a property a listener or the tool above relies on: the
//! length promised, the level and pitch kept, the seed honoured, the
//! channels handled one by one and a bad argument refused by name.

use audio_time::{paulstretch, Error, MAX_PAULSTRETCH_FACTOR, MAX_PAULSTRETCH_WINDOW_SEC};
use realfft::RealFftPlanner;

const RATE: u32 = 48_000;

fn sine(freq: f32, rate: u32, seconds: f32, amp: f32) -> Vec<f32> {
    (0..(rate as f32 * seconds) as usize)
        .map(|n| (2.0 * std::f32::consts::PI * freq * n as f32 / rate as f32).sin() * amp)
        .collect()
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|s| s * s).sum::<f32>() / x.len() as f32).sqrt()
}

/// Frequency of the strongest bin of a Hann-windowed FFT of `x[..n]`.
fn peak_hz(x: &[f32], rate: u32) -> f32 {
    let n = 1 << 15;
    let mut buf: Vec<f32> = x[..n]
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let w = 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / n as f32).cos();
            s * w
        })
        .collect();
    let fft = RealFftPlanner::<f32>::new().plan_fft_forward(n);
    let mut spec = fft.make_output_vec();
    fft.process(&mut buf, &mut spec).unwrap();
    let (bin, _) = spec
        .iter()
        .enumerate()
        .skip(1)
        .max_by(|a, b| a.1.norm().total_cmp(&b.1.norm()))
        .unwrap();
    bin as f32 * rate as f32 / n as f32
}

#[test]
fn output_is_the_input_times_the_factor_long() {
    let x = sine(440.0, RATE, 1.0, 0.5);
    for factor in [1.0_f32, 2.0, 7.5, 20.0] {
        let out = paulstretch(&x, RATE, 1, factor, 0.25, 1).unwrap();
        let want = (x.len() as f64 * factor as f64).round() as usize;
        assert_eq!(out.len(), want, "factor {factor}");
    }
}

#[test]
fn a_steady_tone_keeps_its_level() {
    let x = sine(440.0, RATE, 2.0, 0.5);
    let out = paulstretch(&x, RATE, 1, 8.0, 0.25, 7).unwrap();
    let (before, after) = (rms(&x), rms(&out));
    assert!(
        (after / before - 1.0).abs() < 0.1,
        "rms {before} became {after}"
    );
    // The middle, away from the first and last half windows, is closer still.
    let mid = &out[out.len() / 4..out.len() * 3 / 4];
    assert!((rms(mid) / before - 1.0).abs() < 0.06, "mid {}", rms(mid));
}

#[test]
fn a_sine_stays_at_its_frequency() {
    for freq in [220.0_f32, 1_000.0, 5_000.0] {
        let x = sine(freq, RATE, 1.5, 0.5);
        let out = paulstretch(&x, RATE, 1, 10.0, 0.25, 3).unwrap();
        // Skip the first half window: it is half silence.
        let start = RATE as usize;
        let hz = peak_hz(&out[start..], RATE);
        // One bin of the 2^15 point analysis is 1.46 Hz.
        assert!((hz - freq).abs() < 4.0, "{freq} Hz came out at {hz} Hz");
    }
}

#[test]
fn the_same_seed_gives_the_same_samples_and_another_seed_does_not() {
    let x = sine(330.0, RATE, 0.5, 0.4);
    let a = paulstretch(&x, RATE, 1, 4.0, 0.1, 42).unwrap();
    let b = paulstretch(&x, RATE, 1, 4.0, 0.1, 42).unwrap();
    let c = paulstretch(&x, RATE, 1, 4.0, 0.1, 43).unwrap();
    assert_eq!(a, b);
    assert_ne!(a, c);
}

#[test]
fn stereo_keeps_its_layout_and_each_side_its_own_level() {
    let left = sine(300.0, RATE, 1.0, 0.5);
    let right = sine(900.0, RATE, 1.0, 0.1);
    let inter: Vec<f32> = left
        .iter()
        .zip(&right)
        .flat_map(|(l, r)| [*l, *r])
        .collect();
    let out = paulstretch(&inter, RATE, 2, 6.0, 0.2, 9).unwrap();
    assert_eq!(out.len(), inter.len() * 6);
    let l: Vec<f32> = out.iter().step_by(2).copied().collect();
    let r: Vec<f32> = out.iter().skip(1).step_by(2).copied().collect();
    assert!((rms(&l) / rms(&left) - 1.0).abs() < 0.1, "left {}", rms(&l));
    assert!(
        (rms(&r) / rms(&right) - 1.0).abs() < 0.1,
        "right {}",
        rms(&r)
    );
    assert!((peak_hz(&l[RATE as usize..], RATE) - 300.0).abs() < 4.0);
    assert!((peak_hz(&r[RATE as usize..], RATE) - 900.0).abs() < 4.0);
}

#[test]
fn a_mono_file_is_the_same_whichever_rate_it_has() {
    // 22.05 kHz: the window is counted in seconds, so the pitch and the
    // length in seconds do not depend on the rate.
    let x = sine(500.0, 22_050, 1.0, 0.5);
    let out = paulstretch(&x, 22_050, 1, 5.0, 0.25, 5).unwrap();
    assert_eq!(out.len(), 22_050 * 5);
    assert!((peak_hz(&out[22_050..], 22_050) - 500.0).abs() < 3.0);
}

#[test]
fn silence_stays_silent_and_nothing_is_not_an_error() {
    let out = paulstretch(&vec![0.0; 10_000], RATE, 1, 3.0, 0.1, 1).unwrap();
    assert_eq!(out.len(), 30_000);
    assert!(out.iter().all(|s| *s == 0.0));
    assert!(paulstretch(&[], RATE, 2, 3.0, 0.1, 1).unwrap().is_empty());
}

#[test]
fn input_shorter_than_one_window_still_stretches() {
    let x = sine(440.0, RATE, 0.02, 0.5);
    let out = paulstretch(&x, RATE, 1, 10.0, 1.0, 1).unwrap();
    assert_eq!(out.len(), x.len() * 10);
    assert!(out.iter().all(|s| s.is_finite()));
}

#[test]
fn the_output_never_leaves_full_scale_much_for_a_loud_input() {
    let x = sine(440.0, RATE, 1.0, 0.9);
    let out = paulstretch(&x, RATE, 1, 5.0, 0.25, 1).unwrap();
    assert!(out.iter().all(|s| s.is_finite()));
    // Random phases add up to a noisy signal; the level is the input's, so
    // a few times its peak would be a normalisation fault.
    assert!(out.iter().fold(0.0_f32, |m, s| m.max(s.abs())) < 2.0);
}

#[test]
fn bad_arguments_are_refused_by_name() {
    let x = sine(440.0, RATE, 0.5, 0.5);
    for bad in [
        0.99,
        0.0,
        -2.0,
        MAX_PAULSTRETCH_FACTOR + 0.1,
        f32::NAN,
        f32::INFINITY,
    ] {
        assert!(
            matches!(
                paulstretch(&x, RATE, 1, bad, 0.25, 1),
                Err(Error::InvalidStretch(_))
            ),
            "stretch {bad}"
        );
    }
    for bad in [
        0.0,
        0.019,
        -1.0,
        MAX_PAULSTRETCH_WINDOW_SEC + 0.1,
        f32::NAN,
        f32::INFINITY,
    ] {
        assert!(
            matches!(
                paulstretch(&x, RATE, 1, 4.0, bad, 1),
                Err(Error::InvalidWindow(_))
            ),
            "window {bad}"
        );
    }
    assert!(matches!(
        paulstretch(&x, 0, 1, 4.0, 0.25, 1),
        Err(Error::InvalidSampleRate(0))
    ));
    assert!(matches!(
        paulstretch(&x, RATE, 0, 4.0, 0.25, 1),
        Err(Error::ChannelMismatch(_))
    ));
    assert!(matches!(
        paulstretch(&[0.0; 5], RATE, 2, 4.0, 0.25, 1),
        Err(Error::ChannelMismatch(_))
    ));
}
