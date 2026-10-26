//! All signal generators publish exact, reusable audio before appending a node.

use serde_json::{json, Value};
use std::sync::Mutex;
use tools::{ToolContext, ToolDispatcher, ToolResult};

fn generators() -> [(&'static str, Value); 3] {
    [
        ("generate_click_track", json!({ "bpm": 120, "bars": 1 })),
        (
            "generate_chirp",
            json!({ "start_frequency_hz": 100, "end_frequency_hz": 1000, "duration_sec": 0.1 }),
        ),
        ("generate_dtmf", json!({ "digits": "12#" })),
    ]
}

#[test]
fn generated_audio_is_content_addressed_reused_and_renderable() {
    for (name, args) in generators() {
        let directory = tempfile::tempdir().unwrap();
        let mut store = session::Store::open(directory.path()).unwrap();
        let mut engine = audio_engine::Engine::new();
        let mut clipboard = None;
        let dispatcher = ToolDispatcher::default_dispatcher();
        for expected_index in 0..2 {
            let result = dispatcher
                .invoke(
                    name,
                    args.clone(),
                    &mut ToolContext {
                        store: &mut store,
                        engine: &mut engine,
                        user_message: "",
                        clipboard: &mut clipboard,
                        allowed_tools: None,
                    },
                )
                .unwrap();
            let ToolResult::Ok(output) = result else {
                panic!("{name}: {result:?}");
            };
            assert_eq!(output["track_index"], expected_index, "{name}");
        }
        let state = store.get(store.head().unwrap()).unwrap().state;
        let clip = &state.tracks[0].clips[0];
        let decoded = audio_decoder::decode_file(&clip.source_path).unwrap();
        assert_eq!(decoded.channels, 1, "{name}");
        assert_eq!(decoded.sample_rate, state.sample_rate, "{name}");
        assert_eq!(decoded.samples.len() as u64, clip.length, "{name}");
        let hash = tools::provenance::audio_hash(&decoded.samples, decoded.sample_rate, 1);
        assert_eq!(
            clip.source_path.file_stem().unwrap(),
            hash.as_str(),
            "{name}"
        );
        assert_eq!(
            clip.content_hash,
            Some(tools::provenance::legacy_sample_hash(&decoded.samples)),
            "{name}"
        );
        assert_eq!(
            clip.source_path, state.tracks[1].clips[0].source_path,
            "{name}"
        );
        assert_eq!(
            std::fs::read_dir(directory.path().join("generated"))
                .unwrap()
                .count(),
            1
        );
        assert_eq!(state.length_samples, clip.length, "{name}");

        let export = directory.path().join("generated-mix.wav");
        let result = dispatcher
            .invoke(
                "render_final",
                json!({
                    "node_id": store.head().unwrap().to_hex(),
                    "format": "wav",
                    "out_path": export,
                }),
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
        let rendered = audio_decoder::decode_file(&export).unwrap();
        assert_eq!(rendered.sample_rate, state.sample_rate, "{name}");
        assert_eq!(
            rendered.samples.len() as u64 / u64::from(rendered.channels),
            state.length_samples,
            "{name}"
        );
        assert!(
            rendered.samples.iter().all(|sample| sample.is_finite()),
            "{name}"
        );
        assert!(
            rendered.samples.iter().any(|sample| sample.abs() > 0.001),
            "{name}"
        );
    }
}

#[test]
fn a_publication_failure_does_not_create_a_session_head() {
    for (name, args) in generators() {
        let directory = tempfile::tempdir().unwrap();
        let mut store = session::Store::open(directory.path()).unwrap();
        let mut engine = audio_engine::Engine::new();
        let mut clipboard = None;
        std::fs::write(directory.path().join("generated"), b"not a directory").unwrap();
        let result = ToolDispatcher::default_dispatcher()
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
        assert!(matches!(result, ToolResult::Error(_)), "{name}: {result:?}");
        assert!(store.head().is_none(), "{name}");
    }
}

#[test]
fn staged_dispatch_publishes_the_generated_track_and_reports_its_committed_head() {
    for (name, args) in generators() {
        let directory = tempfile::tempdir().unwrap();
        let store = Mutex::new(session::Store::open(directory.path()).unwrap());
        let dispatcher = Mutex::new(ToolDispatcher::default_dispatcher());
        let engine = Mutex::new(audio_engine::Engine::new());
        let clipboard = Mutex::new(None);
        let dispatched = tools::Shared {
            dispatcher: &dispatcher,
            store: &store,
            engine: &engine,
            clipboard: &clipboard,
        }
        .dispatch(name, args, "", None);
        assert!(
            matches!(dispatched.result, Ok(ToolResult::Ok(_))),
            "{name}: {:?}",
            dispatched.result
        );
        let store = store.lock().unwrap();
        let head = store.head().expect("generated node was committed");
        assert_eq!(dispatched.moved_head, Some(head), "{name}");
        let state = store.get(head).unwrap().state;
        assert_eq!(state.tracks.len(), 1, "{name}");
        let clip = &state.tracks[0].clips[0];
        assert_eq!(state.length_samples, clip.length, "{name}");
        assert!(
            audio_decoder::decode_file(&clip.source_path).is_ok(),
            "{name}"
        );
    }
}

#[test]
fn a_discarded_staged_generation_leaves_its_file_in_the_unswept_generated_folder() {
    for (name, args) in generators() {
        let directory = tempfile::tempdir().unwrap();
        let store = session::Store::open(directory.path()).unwrap();
        let mut staged = store.stage().unwrap();
        let mut engine = audio_engine::Engine::new();
        let mut clipboard = None;
        let result = ToolDispatcher::default_dispatcher()
            .invoke(
                name,
                args,
                &mut ToolContext {
                    store: &mut staged,
                    engine: &mut engine,
                    user_message: "",
                    clipboard: &mut clipboard,
                    allowed_tools: None,
                },
            )
            .unwrap();
        assert!(matches!(result, ToolResult::Ok(_)), "{name}: {result:?}");
        let state = staged.get(staged.head().unwrap()).unwrap().state;
        let path = state.tracks[0].clips[0].source_path.clone();
        assert!(
            path.starts_with(directory.path().join("generated")),
            "{name}"
        );
        assert!(path.is_file(), "{name}");
        // Shared::dispatch discards precisely this staged store on a head race.
        drop(staged);
        assert!(store.head().is_none(), "{name}");
        tools::reclaim::sweep_orphans(&store).unwrap();
        assert!(
            path.is_file(),
            "generated/ is never swept, as for tone and noise: {name}"
        );
    }
}

#[test]
fn committed_generated_audio_is_never_reclaimed() {
    for (name, args) in generators() {
        let directory = tempfile::tempdir().unwrap();
        let mut store = session::Store::open(directory.path()).unwrap();
        let mut engine = audio_engine::Engine::new();
        let mut clipboard = None;
        let dispatcher = ToolDispatcher::default_dispatcher();
        for (tool, args) in [(name, args), ("remove_track", json!({ "track": 0 }))] {
            let result = dispatcher
                .invoke(
                    tool,
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
            assert!(matches!(result, ToolResult::Ok(_)), "{tool}: {result:?}");
        }
        let head = store.get(store.head().unwrap()).unwrap();
        let generated = store.get(head.parent.unwrap()).unwrap();
        let path = &generated.state.tracks[0].clips[0].source_path;
        let original = std::fs::read(path).unwrap();
        tools::reclaim::sweep(&store, 0).unwrap();
        assert!(path.is_file(), "an undo state's audio is kept: {name}");
        assert_eq!(std::fs::read(path).unwrap(), original, "{name}");
    }
}
