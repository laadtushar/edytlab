//! Ramped stretch and pitch shift: the length follows the integral of the
//! ratio, a steady tone's frequency follows the ramp, and a constant ramp
//! is the plain call.

use audio_time::{
    pitch_ramp_output_position, pitch_shift, pitch_shift_ramp, ramp_output_position, time_stretch,
    time_stretch_ramp, Error,
};

fn sine(freq: f32, sr: u32, len: usize) -> Vec<f32> {
    (0..len)
        .map(|n| (2.0 * std::f32::consts::PI * freq * n as f32 / sr as f32).sin() * 0.5)
        .collect()
}

/// Zero-crossing frequency of `x[a..b]`.
fn freq_in(x: &[f32], sr: u32, a: usize, b: usize) -> f32 {
    let s = &x[a..b];
    let c = s
        .windows(2)
        .filter(|w| (w[0] <= 0.0) != (w[1] <= 0.0))
        .count();
    (c as f32 / 2.0) * sr as f32 / s.len() as f32
}

#[test]
fn stretch_length_is_the_integral_of_the_ratio() {
    for (sr, f0, f1) in [
        (48_000u32, 1.0f32, 0.5f32),
        (22_050, 1.0, 2.0),
        (44_100, 2.0, 0.25),
    ] {
        let n = sr as usize;
        let out = time_stretch_ramp(&sine(300.0, sr, n), sr, 1, f0, f1, false).unwrap();
        let want = ramp_output_position(n as f64, n as f64, f0 as f64, f1 as f64);
        assert!(
            (out.len() as f64 - want).abs() <= 1.0,
            "{sr} {f0}->{f1}: {} vs {want}",
            out.len()
        );
    }
}

#[test]
fn stereo_stretch_ramp_is_frame_aligned_and_the_right_length() {
    let sr = 44_100;
    let mono = sine(250.0, sr, sr as usize);
    let st: Vec<f32> = mono.iter().flat_map(|s| [*s, *s * 0.5]).collect();
    let out = time_stretch_ramp(&st, sr, 2, 1.0, 0.5, false).unwrap();
    assert_eq!(out.len() % 2, 0);
    let want = ramp_output_position(sr as f64, sr as f64, 1.0, 0.5);
    assert!((out.len() as f64 / 2.0 - want).abs() <= 1.0);
}

#[test]
fn a_stretch_ramp_keeps_the_pitch() {
    let sr = 44_100;
    let out = time_stretch_ramp(&sine(440.0, sr, sr as usize), sr, 1, 1.0, 0.5, false).unwrap();
    let f = freq_in(&out, sr, out.len() / 4, out.len() * 3 / 4);
    assert!((f - 440.0).abs() < 25.0, "{f}");
}

#[test]
fn constant_ramps_are_the_plain_calls() {
    let sr = 44_100;
    let x = sine(330.0, sr, sr as usize / 2);
    assert_eq!(
        time_stretch_ramp(&x, sr, 1, 0.7, 0.7, false).unwrap(),
        time_stretch(&x, sr, 1, 0.7, false).unwrap()
    );
    assert_eq!(
        pitch_shift_ramp(&x, sr, 1, 3.0, 3.0, true).unwrap(),
        pitch_shift(&x, sr, 1, 3.0, true).unwrap()
    );
}

#[test]
fn pitch_ramp_frequency_follows_the_ramp_at_start_middle_and_end() {
    for sr in [44_100u32, 22_050] {
        let n = sr as usize * 2;
        let out = pitch_shift_ramp(&sine(440.0, sr, n), sr, 1, 0.0, 12.0, false).unwrap();
        assert_eq!(out.len(), n, "duration is preserved");
        let win = sr as usize / 5;
        for centre in [0.2f32, 0.5, 0.8] {
            let c = (n as f32 * centre) as usize;
            let semis = 12.0 * centre;
            let want = 440.0 * 2f32.powf(semis / 12.0);
            let f = freq_in(&out, sr, c - win / 2, c + win / 2);
            assert!(
                (f / want - 1.0).abs() < 0.08,
                "{sr} Hz rate at {centre}: wanted {want}, got {f}"
            );
        }
    }
}

#[test]
fn a_downward_stereo_pitch_ramp_with_formants_keeps_its_length() {
    let sr = 44_100;
    let mono = sine(600.0, sr, sr as usize);
    let st: Vec<f32> = mono.iter().flat_map(|s| [*s, *s]).collect();
    let out = pitch_shift_ramp(&st, sr, 2, 0.0, -12.0, true).unwrap();
    assert_eq!(out.len(), st.len());
    let l: Vec<f32> = out.iter().step_by(2).copied().collect();
    let f = freq_in(&l, sr, l.len() * 7 / 10, l.len() * 9 / 10);
    // Near 80-90 % of the way down: about 8.5 semitones below 600 Hz.
    let want = 600.0 * 2f32.powf(-0.85);
    assert!((f / want - 1.0).abs() < 0.15, "{f} vs {want}");
}

#[test]
fn pitch_position_is_the_integral_of_the_ratio() {
    assert!((pitch_ramp_output_position(50.0, 100.0, 12.0, 12.0) - 100.0).abs() < 1e-9);
    // 0 -> 12 semitones: integral of 2^(t/100) is 100 / ln 2.
    let full = pitch_ramp_output_position(100.0, 100.0, 0.0, 12.0);
    assert!((full - 100.0 / 2f64.ln()).abs() < 1e-9, "{full}");
    assert_eq!(pitch_ramp_output_position(-1.0, 100.0, 0.0, 12.0), 0.0);
}

#[test]
fn refusals_name_the_value() {
    let x = [0.0f32; 8];
    for bad in [0.05f32, 11.0, f32::NAN, f32::INFINITY, -1.0] {
        assert!(matches!(
            time_stretch_ramp(&x, 48_000, 1, bad, 1.0, false),
            Err(Error::InvalidRampFactor(_))
        ));
        assert!(matches!(
            time_stretch_ramp(&x, 48_000, 1, 1.0, bad, false),
            Err(Error::InvalidRampFactor(_))
        ));
    }
    for bad in [49.0f32, -49.0, f32::NAN, f32::NEG_INFINITY] {
        assert!(matches!(
            pitch_shift_ramp(&x, 48_000, 1, bad, 0.0, false),
            Err(Error::InvalidSemitones(_))
        ));
        assert!(matches!(
            pitch_shift_ramp(&x, 48_000, 1, 0.0, bad, false),
            Err(Error::InvalidSemitones(_))
        ));
    }
    assert!(matches!(
        time_stretch_ramp(&x, 48_000, 0, 1.0, 0.5, false),
        Err(Error::ChannelMismatch(_))
    ));
    assert!(matches!(
        pitch_shift_ramp(&x[..7], 48_000, 2, 1.0, 0.5, false),
        Err(Error::ChannelMismatch(_))
    ));
    let e = time_stretch_ramp(&x, 48_000, 1, 0.05, 1.0, false).unwrap_err();
    assert!(e.to_string().contains("0.1 to 10"), "{e}");
}

#[test]
fn empty_input_is_empty_output() {
    assert!(time_stretch_ramp(&[], 48_000, 1, 1.0, 0.5, false)
        .unwrap()
        .is_empty());
    assert!(pitch_shift_ramp(&[], 48_000, 1, 0.0, 5.0, false)
        .unwrap()
        .is_empty());
}

/// A ramp that begins at the identity leaves the head of the signal at
/// its own pitch and does not click: no sample jumps by more than a
/// steady 440 Hz tone's own step.
#[test]
fn a_ramp_from_zero_starts_at_the_original_pitch() {
    let sr = 44_100;
    let out = pitch_shift_ramp(&sine(440.0, sr, sr as usize), sr, 1, 0.0, 2.0, false).unwrap();
    let f = freq_in(&out, sr, sr as usize / 20, sr as usize / 5);
    assert!((f / 440.0 - 1.0).abs() < 0.08, "{f}");
}
