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

    let clipboard = Arc::new(Mutex::new(None::<tools::Clipboard>));

    {
        let d = dispatcher.lock().unwrap();
        let mut s = store.lock().unwrap();
        let mut e = engine.lock().unwrap();
        let mut cb = clipboard.lock().unwrap();
        let mut ctx = tools::ToolContext {
            store: &mut s,
            engine: &mut e,
            user_message: "",
            clipboard: &mut cb,
            allowed_tools: None,
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
    let plan_steps_override = std::sync::Arc::new(std::sync::Mutex::new(None));
    let mut agent = ai::Agent::new(
        cfg,
        dispatcher,
        store,
        engine,
        plan_notify,
        plan_steps_override,
        Arc::new(std::sync::atomic::AtomicBool::new(false)),
        clipboard,
    );

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

/// The key for the live tests: `ANTHROPIC_API_KEY`, or `ANTHROPIC_E2E_KEY`,
/// which `live_thinking_binding` reads.
fn live_key() -> Option<String> {
    std::env::var("ANTHROPIC_API_KEY")
        .or_else(|_| std::env::var("ANTHROPIC_E2E_KEY"))
        .ok()
        .filter(|k| !k.trim().is_empty())
}

/// What Settings lists for Anthropic with a saved key is Anthropic's own
/// list, in the shape `fetch_anthropic_models` reads, and it holds the two
/// models the app uses without being asked. The mocked tests pin the shape
/// the docs describe; this is the check that the real API still answers in
/// it, and that the defaults still exist on it.
#[tokio::test]
#[ignore = "live API; requires ANTHROPIC_API_KEY"]
async fn live_model_list_holds_the_defaults() {
    let Some(key) = live_key() else {
        eprintln!("no key set, skipping");
        return;
    };

    ai::models::clear_cache();
    let catalogue = ai::list_models_for_at(ai::ANTHROPIC_ID, Some(&key), None)
        .await
        .expect("the catalogue never errors for Anthropic");

    assert_eq!(
        catalogue.source,
        ai::CatalogueSource::Live,
        "the live list was not used"
    );
    let ids: Vec<&str> = catalogue.models.iter().map(|m| m.id.as_str()).collect();
    eprintln!("live Anthropic models: {ids:?}");
    assert!(ids.contains(&ai::DEFAULT_MODEL), "{ids:?}");
    assert!(ids.contains(&ai::CLASSIFIER_MODEL), "{ids:?}");
    for m in &catalogue.models {
        assert!(
            !m.display_name.is_empty() && m.context_length.is_some(),
            "{m:?}: the live entry is missing a name or a context length"
        );
    }
}
