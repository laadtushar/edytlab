use serde_json::json;
use tools::{ToolContext, ToolDispatcher, ToolResult};

#[test]
fn wah_wah_appends_a_filtered_node() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = session::Store::open(dir.path()).unwrap();
    let mut engine = audio_engine::Engine::new();
    let mut clipboard = None;
    let dispatcher = ToolDispatcher::default_dispatcher();
    for (name, args) in [
        (
            "generate_tone",
            json!({"frequency_hz": 4000, "duration_sec": 1.0}),
        ),
        (
            "wah_wah",
            json!({"track": 0, "rate_hz": 1.0, "depth": 1.0, "base_freq_hz": 1000.0, "resonance": 2.0}),
        ),
    ] {
        let result = dispatcher
            .invoke(
                name,
                args,
                &mut ToolContext {
                    store: &mut store,
                    engine: &mut engine,
                    user_message: "",
                    clipboard: &mut clipboard,
                    allowed_tools: None,
                },
            )
            .unwrap();
        assert!(matches!(result, ToolResult::Ok(_)), "{name}: {result:?}");
        if name == "generate_tone" {
            let before = store.head().unwrap();
            let source = store.get(before).unwrap().state.tracks[0].clips[0]
                .source_path
                .clone();
            assert!(source.exists());
        }
    }
    let state = store.get(store.head().unwrap()).unwrap().state;
    let output = audio_decoder::decode_file(&state.tracks[0].clips[0].source_path).unwrap();
    assert!(output.samples.iter().any(|s| s.abs() > 0.01));
    let n = output.sample_rate as usize;
    let rms = |start: usize| {
        (output.samples[start..start + n / 20]
            .iter()
            .map(|s| s * s)
            .sum::<f32>()
            / (n / 20) as f32)
            .sqrt()
    };
    assert!(rms(n / 2 - n / 40) < rms(n / 20));
}

/// The base is held to 200..5000 Hz, an infinity included, so the label
/// and the filter never see `inf` or a base low enough to go unstable.
#[test]
fn wah_wah_clamps_the_base_and_names_the_clamped_value() {
    for (base, shown) in [(1e39, "base=5000.0Hz"), (1.0, "base=200.0Hz")] {
        let dir = tempfile::tempdir().unwrap();
        let mut store = session::Store::open(dir.path()).unwrap();
        let mut engine = audio_engine::Engine::new();
        let mut clipboard = None;
        let dispatcher = ToolDispatcher::default_dispatcher();
        let mut summary = String::new();
        for (name, args) in [
            (
                "generate_tone",
                json!({"frequency_hz": 440, "duration_sec": 0.2}),
            ),
            ("wah_wah", json!({"track": 0, "base_freq_hz": base})),
        ] {
            let result = dispatcher
                .invoke(
                    name,
                    args,
                    &mut ToolContext {
                        store: &mut store,
                        engine: &mut engine,
                        user_message: "",
                        clipboard: &mut clipboard,
                        allowed_tools: None,
                    },
                )
                .unwrap();
            let ToolResult::Ok(value) = result else {
                panic!("{name}: {result:?}");
            };
            if name == "wah_wah" {
                summary = value["summary"].as_str().unwrap().to_owned();
            }
        }
        assert!(summary.contains(shown), "{summary}");
        assert!(!summary.contains("inf"), "{summary}");
    }
}
