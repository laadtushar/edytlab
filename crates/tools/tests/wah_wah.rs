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
