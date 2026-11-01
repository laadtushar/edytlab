//! End-to-end coverage for generated DTMF audio and session changes.

use serde_json::{json, Value};
use tools::ToolResult;

#[path = "support/harness.rs"]
mod harness;
use harness::Session;

impl Session {
    fn try_dtmf(&mut self, args: Value) -> Result<ToolResult, tools::DispatchError> {
        self.try_call("generate_dtmf", args)
    }

    fn dtmf(&mut self, args: Value) -> Value {
        self.ok("generate_dtmf", args)
    }
}

fn read_wav(path: &std::path::Path) -> (u32, Vec<f64>) {
    let mut reader = hound::WavReader::open(path).expect("generated wav opens");
    let rate = reader.spec().sample_rate;
    let samples = reader
        .samples::<f32>()
        .map(|sample| sample.unwrap() as f64)
        .collect();
    (rate, samples)
}

fn goertzel_power(samples: &[f64], sample_rate: u32, frequency: f64) -> f64 {
    let omega = std::f64::consts::TAU * frequency / sample_rate as f64;
    let coeff = 2.0 * omega.cos();
    let (mut q1, mut q2) = (0.0, 0.0);
    for &sample in samples {
        let q0 = sample + coeff * q1 - q2;
        q2 = q1;
        q1 = q0;
    }
    q1 * q1 + q2 * q2 - coeff * q1 * q2
}

fn keypad_frequencies(digit: char) -> (f64, f64) {
    let row = match digit {
        '1' | '2' | '3' | 'A' => 697.0,
        '4' | '5' | '6' | 'B' => 770.0,
        '7' | '8' | '9' | 'C' => 852.0,
        '0' | '*' | '#' | 'D' => 941.0,
        _ => panic!("unexpected keypad digit {digit}"),
    };
    let column = match digit {
        '1' | '4' | '7' | '*' => 1209.0,
        '2' | '5' | '8' | '0' => 1336.0,
        '3' | '6' | '9' | '#' => 1477.0,
        'A' | 'B' | 'C' | 'D' => 1633.0,
        _ => unreachable!(),
    };
    (row, column)
}

#[test]
fn every_keypad_digit_contains_its_two_standard_frequencies() {
    let digits = "0123456789ABCD*#";
    let tone_duration = 0.1;
    let gap_duration = 0.05;
    let mut project = Session::empty();
    let result = project.dtmf(json!({
        "digits": digits,
        "tone_duration_sec": tone_duration,
        "gap_duration_sec": gap_duration,
        "amplitude": 0.6
    }));
    assert_eq!(result["track_index"], 0);

    let state = project.state();
    assert_eq!(state.tracks.len(), 1);
    let clip = &state.tracks[0].clips[0];
    assert_eq!(clip.length, state.length_samples);
    let (sample_rate, samples) = read_wav(&clip.source_path);
    assert_eq!(sample_rate, state.sample_rate);
    let tone_frames = (sample_rate as f64 * tone_duration).round() as usize;
    let gap_frames = (sample_rate as f64 * gap_duration).round() as usize;
    assert_eq!(
        samples.len(),
        digits.len() * tone_frames + (digits.len() - 1) * gap_frames
    );
    assert_eq!(state.length_samples as usize, samples.len());

    for (index, digit) in digits.chars().enumerate() {
        let tone_start = index * (tone_frames + gap_frames);
        let window_start = tone_start + (sample_rate as f64 * 0.025) as usize;
        let window_end = tone_start + tone_frames - (sample_rate as f64 * 0.025) as usize;
        let window = &samples[window_start..window_end];
        let (row, column) = keypad_frequencies(digit);
        let row_power = goertzel_power(window, sample_rate, row);
        let column_power = goertzel_power(window, sample_rate, column);
        let row_other = [697.0, 770.0, 852.0, 941.0]
            .into_iter()
            .filter(|&frequency| frequency != row)
            .map(|frequency| goertzel_power(window, sample_rate, frequency))
            .fold(0.0, f64::max);
        let column_other = [1209.0, 1336.0, 1477.0, 1633.0]
            .into_iter()
            .filter(|&frequency| frequency != column)
            .map(|frequency| goertzel_power(window, sample_rate, frequency))
            .fold(0.0, f64::max);
        assert!(
            row_power > 1.0,
            "{digit}: expected row tone {row}Hz, got {row_power}"
        );
        assert!(
            column_power > 1.0,
            "{digit}: expected column tone {column}Hz, got {column_power}"
        );
        assert!(
            row_power > row_other * 8.0,
            "{digit}: weak row-tone separation"
        );
        assert!(
            column_power > column_other * 8.0,
            "{digit}: weak column-tone separation"
        );
        assert!(samples[tone_start..tone_start + tone_frames]
            .iter()
            .all(|sample| sample.abs() <= 0.61));

        if index + 1 < digits.len() {
            let gap = &samples[tone_start + tone_frames..tone_start + tone_frames + gap_frames];
            assert!(
                gap.iter().all(|sample| sample.abs() < 0.0001),
                "gap after {digit} is not silent"
            );
        }
    }
}

#[test]
fn default_durations_and_second_call_append_tracks() {
    let mut project = Session::empty();
    project.dtmf(json!({ "digits": "12" }));
    let old_head = project.store.head().unwrap();
    let first = project.state();
    assert_eq!(first.length_samples, 2 * 4410 + 2205);

    let result = project.dtmf(json!({ "digits": "aD" }));
    assert_eq!(result["track_index"], 1);
    assert_ne!(project.store.head().unwrap(), old_head);
    let after = project.state();
    assert_eq!(after.tracks.len(), 2);
    assert_eq!(after.length_samples, first.length_samples);
    assert_eq!(after.tracks[1].name, "DTMF AD");
}

#[test]
fn rejects_tones_shorter_than_40ms_without_creating_audio_or_a_head() {
    let mut project = Session::empty();
    let result = project.try_dtmf(json!({
        "digits": "12",
        "tone_duration_sec": 0.039,
        "gap_duration_sec": 0.0
    }));
    assert!(
        matches!(result, Err(_) | Ok(ToolResult::Error(_))),
        "sub-minimum tone should be rejected: {result:?}"
    );
    assert!(project.store.head().is_none());
    assert!(!project.dir.path().join("generated").exists());
}

#[test]
fn minimum_40ms_tones_contain_both_dtmf_frequencies() {
    let mut project = Session::empty();
    project.dtmf(json!({
        "digits": "12",
        "tone_duration_sec": 0.04,
        "gap_duration_sec": 0.0
    }));
    let state = project.state();
    let (sample_rate, samples) = read_wav(&state.tracks[0].clips[0].source_path);
    assert_eq!(
        samples.len(),
        (sample_rate as f64 * 0.04).round() as usize * 2
    );
    for (index, digit) in "12".chars().enumerate() {
        let start = index * (sample_rate as f64 * 0.04).round() as usize;
        let window = &samples[start + 441..start + 1323];
        let (row, column) = keypad_frequencies(digit);
        assert!(goertzel_power(window, sample_rate, row) > 1.0);
        assert!(goertzel_power(window, sample_rate, column) > 1.0);
    }
}

#[test]
fn invalid_inputs_return_errors_without_changing_the_head() {
    for args in [
        json!({ "digits": "" }),
        json!({ "digits": "12x" }),
        json!({ "digits": "12", "tone_duration_sec": 0.0 }),
        json!({ "digits": "12", "tone_duration_sec": -0.1 }),
        json!({ "digits": "12", "gap_duration_sec": -0.01 }),
        json!({ "digits": "12", "amplitude": 1.01 }),
        json!({ "digits": "12", "amplitude": -0.01 }),
        json!({ "digits": "1", "tone_duration_sec": 601.0 }),
        json!({ "digits": "12", "tone_duration_sec": 400.0 }),
    ] {
        let mut project = Session::empty();
        assert!(project.store.head().is_none());
        match project.try_dtmf(args.clone()) {
            Err(_) | Ok(ToolResult::Error(_)) => {}
            Ok(ToolResult::Ok(result)) => {
                panic!("invalid arguments succeeded: {args} -> {result}")
            }
        }
        assert!(
            project.store.head().is_none(),
            "invalid arguments changed head: {args}"
        );
    }

    let mut project = Session::empty();
    project.dtmf(json!({ "digits": "123" }));
    let before = project.store.head().unwrap();
    match project.try_dtmf(json!({ "digits": "12x" })).unwrap() {
        ToolResult::Error(_) => {}
        ToolResult::Ok(result) => panic!("invalid digits succeeded: {result}"),
    }
    assert_eq!(project.store.head(), Some(before));
}

#[test]
fn long_digit_strings_are_truncated_in_the_name_and_a_tiny_gap_is_none() {
    let mut p = Session::empty();
    let digits = "1234567890".repeat(4);
    p.dtmf(json!({ "digits": digits, "tone_duration_sec": 0.04, "gap_duration_sec": 0.000001 }));
    let state = p.state();
    assert_eq!(state.tracks[0].name, format!("DTMF {}…", &digits[..32]));
    let rate = u64::from(state.sample_rate);
    let tone = (rate as f64 * 0.04).round() as u64;
    assert_eq!(
        state.tracks[0].clips[0].length,
        tone * 40,
        "the gap rounds to zero frames"
    );
}
