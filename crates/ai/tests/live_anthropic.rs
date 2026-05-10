//! Live integration test against the real Anthropic API.
//!
//! Marked `#[ignore]` so the regular `cargo test` run skips it; CI does
//! not have an API key. To run locally:
//!
//! ```bash
//! ANTHROPIC_API_KEY=sk-ant-... cargo test -p ai --test live_anthropic -- --ignored --nocapture
//! ```
//!
//! The test loads a tiny WAV, sends a single user turn, and just checks
//! that the agent comes back without an error and emits at least one
//! event. We deliberately do NOT assert on the exact tool sequence
//! the model picks — that's non-deterministic.

use std::sync::{Arc, Mutex};

use serde_json::json;

#[tokio::test]
#[ignore = "live API; requires ANTHROPIC_API_KEY"]
async fn live_round_trip() {
    let api_key = match std::env::var("ANTHROPIC_API_KEY") {
        Ok(k) => k,
        Err(_) => {
            eprintln!("ANTHROPIC_API_KEY not set, skipping");
            return;
        }
    };

    let project = tempfile::tempdir().unwrap();
    let wav_path = project.path().join("input.wav");
    {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 44100,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(&wav_path, spec).unwrap();
        for i in 0..4410 {
            let t = i as f32 / 44100.0;
            let s = (t * 440.0 * std::f32::consts::TAU).sin() * 0.5;
            writer.write_sample((s * i16::MAX as f32) as i16).unwrap();
        }
        writer.finalize().unwrap();
    }

    let dispatcher = Arc::new(Mutex::new(tools::ToolDispatcher::default_dispatcher()));
    let store = Arc::new(Mutex::new(session::Store::open(project.path()).unwrap()));
    let engine = Arc::new(Mutex::new(audio_engine::Engine::new()));

    {
        let d = dispatcher.lock().unwrap();
        let mut s = store.lock().unwrap();
        let mut e = engine.lock().unwrap();
        let mut ctx = tools::ToolContext {
            store: &mut s,
            engine: &mut e,
            user_message: "",
            clipboard: &mut clipboard,
        };
        d.invoke(
            "load",
            json!({ "path": wav_path.to_string_lossy() }),
            &mut ctx,
        )
        .unwrap();
    }

    let cfg = ai::LlmConfig::new_anthropic(api_key);
    let plan_notify = std::sync::Arc::new(tokio::sync::Notify::new());
    let mut agent = ai::Agent::new(cfg, dispatcher, store, engine, plan_notify);

    let mut events = 0usize;
    let result = agent
        .turn("normalize this to -1 dBFS".to_string(), |ev| {
            eprintln!("event: {ev:?}");
            events += 1;
        })
        .await
        .expect("turn failed");

    assert!(events > 0, "expected at least one event");
    eprintln!("final text: {}", result.text);
}
