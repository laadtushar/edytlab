use serde_json::{json, Value};
use tempfile::TempDir;
use tools::{ToolContext, ToolDispatcher, ToolResult};

struct Project {
    _dir: TempDir,
    store: session::Store,
    engine: audio_engine::Engine,
    dispatcher: ToolDispatcher,
    clipboard: Option<tools::Clipboard>,
}

impl Project {
    fn empty() -> Self {
        let dir = TempDir::new().unwrap();
        let store = session::Store::open(dir.path()).unwrap();
        Self {
            _dir: dir,
            store,
            engine: audio_engine::Engine::new(),
            dispatcher: ToolDispatcher::default_dispatcher(),
            clipboard: None,
        }
    }

    fn invoke(&mut self, args: Value) -> Result<ToolResult, tools::DispatchError> {
        let mut ctx = ToolContext {
            store: &mut self.store,
            engine: &mut self.engine,
            user_message: "",
            clipboard: &mut self.clipboard,
            allowed_tools: None,
        };
        self.dispatcher.invoke("generate_chirp", args, &mut ctx)
    }

    fn ok(&mut self, args: Value) -> Value {
        match self.invoke(args).unwrap() {
            ToolResult::Ok(value) => value,
            ToolResult::Error(message) => panic!("generate_chirp: {message}"),
        }
    }

    fn state(&self) -> session::SessionState {
        self.store
            .get(self.store.head().expect("a head"))
            .unwrap()
            .state
    }

    fn samples(&self) -> (u32, Vec<f32>) {
        let state = self.state();
        let path = &state.tracks.last().unwrap().clips[0].source_path;
        let mut reader = hound::WavReader::open(path).unwrap();
        let spec = reader.spec();
        let samples = match spec.sample_format {
            hound::SampleFormat::Float => reader.samples::<f32>().map(Result::unwrap).collect(),
            hound::SampleFormat::Int if spec.bits_per_sample == 16 => reader
                .samples::<i16>()
                .map(|sample| sample.unwrap() as f32 / i16::MAX as f32)
                .collect(),
            other => panic!("unexpected generated WAV format: {other:?}"),
        };
        (spec.sample_rate, samples)
    }
}

fn zero_crossing_frequency(samples: &[f32], sample_rate: u32, at_seconds: f64) -> f64 {
    let crossings: Vec<f64> = samples
        .windows(2)
        .enumerate()
        .filter_map(|(index, pair)| {
            if pair[0] <= 0.0 && pair[1] > 0.0 {
                let fraction = -pair[0] as f64 / (pair[1] - pair[0]) as f64;
                Some((index as f64 + fraction) / sample_rate as f64)
            } else {
                None
            }
        })
        .collect();
    let closest_period = crossings
        .windows(2)
        .min_by(|left, right| {
            let left_midpoint = (left[0] + left[1]) / 2.0;
            let right_midpoint = (right[0] + right[1]) / 2.0;
            (left_midpoint - at_seconds)
                .abs()
                .total_cmp(&(right_midpoint - at_seconds).abs())
        })
        .expect("generated sweep has adjacent upward zero crossings");
    1.0 / (closest_period[1] - closest_period[0])
}

#[test]
fn linear_and_logarithmic_wavs_follow_the_requested_frequency_curve() {
    let start = 160.0;
    let end = 1_600.0;
    let duration = 2.0;
    for method in ["linear", "logarithmic"] {
        let mut project = Project::empty();
        project.ok(json!({
            "start_frequency_hz": start,
            "end_frequency_hz": end,
            "duration_sec": duration,
            "method": method,
        }));
        let (rate, samples) = project.samples();
        assert_eq!(rate, project.state().sample_rate);
        let expected_frequencies = if method == "linear" {
            [160.0, 880.0, 1_600.0]
        } else {
            [160.0, 505.964_425_626_940_7, 1_600.0]
        };
        for (t, expected) in [0.0_f64, 1.0, 2.0].into_iter().zip(expected_frequencies) {
            let observed = zero_crossing_frequency(&samples, rate, t);
            assert!(
                (observed - expected).abs() < 10.0,
                "{method} at {t}s: expected {expected:.1}Hz, observed {observed:.1}Hz"
            );
        }
    }
}

#[test]
fn equal_endpoints_make_a_constant_frequency_tone_and_defaults_apply() {
    let mut project = Project::empty();
    project.ok(json!({
        "start_frequency_hz": 440.0,
        "end_frequency_hz": 440.0,
        "duration_sec": 0.5,
    }));
    let (rate, samples) = project.samples();
    assert_eq!(samples.len(), (rate as f64 * 0.5) as usize);
    for t in [0.1_f64, 0.25, 0.4] {
        let observed = zero_crossing_frequency(&samples, rate, t);
        assert!((observed - 440.0).abs() < 12.0, "observed {observed:.1}Hz");
    }
    let peak = samples
        .iter()
        .map(|sample| sample.abs())
        .fold(0.0, f32::max);
    assert!((peak - 0.5).abs() < 0.02, "default amplitude peak {peak}");
}

#[test]
fn extreme_positive_logarithmic_sweeps_stay_finite_in_both_directions() {
    for (start, end) in [(1e-300, 1_000.0), (1_000.0, 1e-300)] {
        let mut project = Project::empty();
        project.ok(json!({
            "start_frequency_hz": start,
            "end_frequency_hz": end,
            "duration_sec": 0.01,
            "method": "logarithmic",
        }));
        let (_, samples) = project.samples();
        assert!(samples.iter().all(|sample| sample.is_finite()));
    }
}

#[test]
fn explicit_amplitude_controls_peak_and_sessions_append_at_the_existing_rate() {
    let mut project = Project::empty();
    let first = project.ok(json!({
        "start_frequency_hz": 100.0,
        "end_frequency_hz": 100.0,
        "duration_sec": 0.25,
        "amplitude": 0.2,
    }));
    assert_eq!(first["track_index"], 0);
    let original_rate = project.state().sample_rate;
    let first_head = project.store.head().unwrap();
    let second = project.ok(json!({
        "start_frequency_hz": 200.0,
        "end_frequency_hz": 800.0,
        "duration_sec": 0.5,
        "amplitude": 0.8,
        "method": "logarithmic",
    }));
    assert_eq!(second["track_index"], 1);
    let node = project.store.get(project.store.head().unwrap()).unwrap();
    assert_eq!(node.parent, Some(first_head));
    let state = node.state;
    assert_eq!(state.sample_rate, original_rate);
    assert_eq!(state.tracks.len(), 2);
    assert_eq!(state.length_samples, (original_rate as f64 * 0.5) as u64);
    assert!(state
        .tracks
        .iter()
        .all(|track| track.clips[0].source_path.exists()));
    let (rate, samples) = project.samples();
    assert_eq!(rate, original_rate);
    let peak = samples
        .iter()
        .map(|sample| sample.abs())
        .fold(0.0, f32::max);
    assert!((peak - 0.8).abs() < 0.02, "explicit amplitude peak {peak}");
}

#[test]
fn invalid_values_are_rejected_without_creating_a_session_head() {
    let invalid = [
        json!({"start_frequency_hz":0.0,"end_frequency_hz":440.0,"duration_sec":1.0}),
        json!({"start_frequency_hz":440.0,"end_frequency_hz":0.0,"duration_sec":1.0}),
        json!({"start_frequency_hz":440.0,"end_frequency_hz":44100.0,"duration_sec":1.0}),
        json!({"start_frequency_hz":440.0,"end_frequency_hz":880.0,"duration_sec":0.0}),
        json!({"start_frequency_hz":440.0,"end_frequency_hz":880.0,"duration_sec":601.0}),
        json!({"start_frequency_hz":440.0,"end_frequency_hz":880.0,"duration_sec":1.0,"amplitude":-0.1}),
        json!({"start_frequency_hz":440.0,"end_frequency_hz":880.0,"duration_sec":1.0,"amplitude":1.1}),
        json!({"start_frequency_hz":440.0,"end_frequency_hz":880.0,"duration_sec":1.0,"method":"quadratic"}),
    ];
    for args in invalid {
        let mut project = Project::empty();
        let outcome = project.invoke(args);
        assert!(matches!(
            outcome,
            Ok(ToolResult::Error(_)) | Err(tools::DispatchError::SchemaValidation(_))
        ));
        assert!(project.store.head().is_none());
    }
}

#[test]
fn non_finite_and_unrepresentable_lengths_fail_before_generation() {
    for args in [
        json!({"start_frequency_hz":440.0,"end_frequency_hz":880.0,"duration_sec":1e308}),
        json!({"start_frequency_hz":1e308,"end_frequency_hz":880.0,"duration_sec":1.0}),
        json!({"start_frequency_hz":440.0,"end_frequency_hz":880.0,"duration_sec":1.0,"amplitude":1e308}),
    ] {
        let mut project = Project::empty();
        let outcome = project.invoke(args);
        assert!(matches!(
            outcome,
            Ok(ToolResult::Error(_)) | Err(tools::DispatchError::SchemaValidation(_))
        ));
        assert!(project.store.head().is_none());
    }
}
