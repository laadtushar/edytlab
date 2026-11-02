//! Acceptance checks for #769, through the real dispatcher and audio store.
use serde_json::{json, Value};
use tools::{ToolContext, ToolDispatcher, ToolResult};

const SR: u32 = 44_100;

struct Rig {
    dir: tempfile::TempDir,
    store: session::Store,
    engine: audio_engine::Engine,
    clipboard: Option<tools::Clipboard>,
}
impl Rig {
    fn new(samples: &[f32], channels: u16) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("input.wav");
        let mut wav = hound::WavWriter::create(
            &path,
            hound::WavSpec {
                channels,
                sample_rate: SR,
                bits_per_sample: 32,
                sample_format: hound::SampleFormat::Float,
            },
        )
        .unwrap();
        for &s in samples {
            wav.write_sample(s).unwrap();
        }
        wav.finalize().unwrap();
        let mut rig = Self {
            store: session::Store::open(dir.path()).unwrap(),
            dir,
            engine: audio_engine::Engine::new(),
            clipboard: None,
        };
        rig.ok("load", json!({"path": path}));
        rig
    }
    fn call(&mut self, name: &str, args: Value) -> ToolResult {
        ToolDispatcher::default_dispatcher()
            .invoke(
                name,
                args,
                &mut ToolContext {
                    store: &mut self.store,
                    engine: &mut self.engine,
                    user_message: "",
                    clipboard: &mut self.clipboard,
                    allowed_tools: None,
                },
            )
            .unwrap_or_else(|e| ToolResult::Error(e.to_string()))
    }
    fn ok(&mut self, name: &str, args: Value) -> Value {
        match self.call(name, args) {
            ToolResult::Ok(v) => v,
            ToolResult::Error(e) => panic!("{name}: {e}"),
        }
    }
    fn path(&self) -> std::path::PathBuf {
        self.store
            .get(self.store.head().unwrap())
            .unwrap()
            .state
            .tracks[0]
            .clips[0]
            .source_path
            .clone()
    }
    fn samples(&self) -> Vec<f32> {
        audio_decoder::decode_file(&self.path()).unwrap().samples
    }
}
fn sine(freq: f32, amplitude: f32) -> Vec<f32> {
    (0..SR as usize)
        .map(|i| amplitude * (std::f32::consts::TAU * freq * i as f32 / SR as f32).sin())
        .collect()
}
fn rms_error(a: &[f32], b: &[f32]) -> f32 {
    (a.iter().zip(b).map(|(x, y)| (x - y).powi(2)).sum::<f32>() / a.len() as f32).sqrt()
}
#[test]
fn declip_reconstructs_a_sine_clipped_at_minus_three_dbfs() {
    let original = sine(440.0, 0.95);
    let level = 10.0f32.powf(-3.0 / 20.0);
    let clipped: Vec<_> = original.iter().map(|s| s.clamp(-level, level)).collect();
    let mut rig = Rig::new(&clipped, 1);
    let head = rig.store.head().unwrap();
    let result = rig.ok("declip", json!({"track":0,"threshold_dbfs":-3.0}));
    let error = rms_error(&rig.samples(), &original);
    eprintln!("declip RMS error: {error}");
    assert!(error < 0.003, "RMS error {error} must be below -50 dBFS");
    assert!(result["runs_found"].as_u64().unwrap() > 800);
    assert_eq!(result["runs_found"], result["runs_repaired"]);
    assert_ne!(head, rig.store.head().unwrap());
    assert!(rig.path().starts_with(rig.dir.path()));
}
#[test]
fn unclipped_audio_is_bit_identical_and_returns_the_existing_node() {
    let mut rig = Rig::new(&sine(440.0, 0.5), 1);
    let path = rig.path();
    let bytes = std::fs::read(&path).unwrap();
    let head = rig.store.head().unwrap();
    let result = rig.ok("declip", json!({"track":0}));
    assert_eq!(result["runs_found"], 0);
    assert_eq!(rig.path(), path);
    assert_eq!(std::fs::read(rig.path()).unwrap(), bytes);
    assert_eq!(rig.store.head().unwrap(), head);
}
#[test]
fn twenty_ms_sine_gap_has_no_boundary_click_and_keeps_stereo_separate() {
    let left = sine(440.0, 0.5);
    let right = sine(220.0, 0.25);
    let original: Vec<_> = left
        .iter()
        .zip(&right)
        .flat_map(|(&a, &b)| [a, b])
        .collect();
    let (a, b) = (8_820, 9_702);
    let mut broken = original.clone();
    broken[a * 2..b * 2].fill(0.0);
    let mut rig = Rig::new(&broken, 2);
    rig.ok(
        "repair_span",
        json!({"track":0,"start_sec":0.2,"end_sec":0.22}),
    );
    let output = rig.samples();
    assert_eq!(output.len(), broken.len());
    // No boundary step above -28 dBFS (0.0399), including the sine's own slope.
    for ch in 0..2 {
        for edge in [a, b] {
            let step = (output[edge * 2 + ch] - output[(edge - 1) * 2 + ch]).abs();
            assert!(step < 0.04, "boundary step {step}");
        }
    }
    let error = rms_error(&output[a * 2..b * 2], &original[a * 2..b * 2]);
    eprintln!("repair_span gap RMS error: {error}");
    assert!(error < 0.001, "gap RMS error {error}");
    assert!(rms_error(&output[..a * 2], &original[..a * 2]) < 0.0001);
    assert!(rms_error(&output[b * 2..], &original[b * 2..]) < 0.0001);
}
#[test]
fn invalid_spans_leave_the_head_and_audio_unchanged() {
    let mut rig = Rig::new(&sine(440.0, 0.5), 1);
    let head = rig.store.head().unwrap();
    for (start, end) in [
        (-0.1, 0.0),
        (0.3, 0.2),
        (0.2, 0.2),
        (0.2, 0.301),
        (0.0, 0.02),
        (0.99, 1.01),
    ] {
        assert!(matches!(
            rig.call(
                "repair_span",
                json!({"track":0,"start_sec":start,"end_sec":end})
            ),
            ToolResult::Error(_)
        ));
        assert_eq!(rig.store.head().unwrap(), head);
    }
}

#[test]
fn a_smooth_full_scale_sine_is_not_mistaken_for_a_clipped_plateau() {
    let mut rig = Rig::new(&sine(50.0, 1.0), 1);
    let before = std::fs::read(rig.path()).unwrap();
    let result = rig.ok("declip", json!({"track":0}));
    assert_eq!(result["runs_repaired"], 0);
    assert_eq!(result["runs_found"], 0);
    assert_eq!(std::fs::read(rig.path()).unwrap(), before);
}

#[test]
fn full_scale_clipping_is_repaired_with_reported_headroom() {
    let original = sine(440.0, 1.2);
    let clipped: Vec<_> = original.iter().map(|s| s.clamp(-1.0, 1.0)).collect();
    let mut rig = Rig::new(&clipped, 1);
    let result = rig.ok("declip", json!({"track":0}));
    let gain = 10.0f32.powf(result["gain_db"].as_f64().unwrap() as f32 / 20.0);
    assert!(gain < 1.0);
    assert!(result["runs_repaired"].as_u64().unwrap() > 800);
    let expected: Vec<_> = original.iter().map(|s| s * gain).collect();
    let error = rms_error(&rig.samples(), &expected);
    eprintln!("full-scale declip RMS error {error}, report {result}");
    assert!(
        error < 0.01,
        "full-scale declip RMS error {error}, report {result}"
    );
}
