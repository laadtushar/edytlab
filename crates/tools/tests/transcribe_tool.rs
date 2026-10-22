//! The `transcribe` tool, against a recogniser that is not a model (#384).
//!
//! What the tool does around the model: which audio it reads, the node it
//! appends and what that node says about where the words came from, the cache,
//! the language, the answers to every way it can fail, and the result the
//! model reads. The real model is exercised by `transcribe_end_to_end.rs`
//! (`#[ignore]`d: it needs the model and ONNX Runtime).

#[path = "support/speech.rs"]
mod speech;

use std::sync::Arc;

use serde_json::{json, Value};
use session::TranscriptSource;
use speech::{model_ref, tone, word, Fake, Project};
use tools::tool::transcribe::{
    SpeechError, MARKERS, MARKER_MODEL_DAMAGED, MARKER_MODEL_NOT_DOWNLOADED,
    MARKER_MODEL_PATH_INVALID, MARKER_RUNTIME_UNAVAILABLE,
};

fn hello() -> Vec<session::TranscriptWord> {
    vec![
        word("Hello,", 0.30, 0.84),
        word("um", 0.90, 1.10),
        word("world.", 1.20, 1.80),
    ]
}

fn project() -> (Arc<Fake>, Project) {
    let fake = Arc::new(Fake::new(hello()));
    let mut p = Project::new(Arc::clone(&fake) as Arc<dyn tools::tool::transcribe::SpeechToText>);
    p.load("a.wav", &tone(2.0, 220.0));
    p.load("b.wav", &tone(2.0, 330.0));
    (fake, p)
}

// ---------------------------------------------------------------------
// The node
// ---------------------------------------------------------------------

#[test]
fn a_track_is_transcribed_into_a_new_node_that_names_the_track() {
    let (fake, mut p) = project();
    let before = p.node_count();
    let track_id = p.state().tracks[1].id.clone();

    let v = p.ok("transcribe", json!({ "track": 1 }));

    assert_eq!(p.node_count(), before + 1, "one node");
    let t = p.state().transcript.expect("the node carries a transcript");
    assert_eq!(t.words.len(), 3);
    assert_eq!(t.words[1].text, "um");
    assert_eq!(t.source, Some(TranscriptSource::Track { track: track_id }));
    assert_eq!(fake.runs(), 1);

    assert_eq!(v["word_count"], 3);
    assert_eq!(v["language"], "en");
    assert_eq!(v["cached"], false);
    assert_eq!(v["replaced_earlier_transcript"], false);
    assert_eq!(
        v["words"],
        json!([
            ["Hello,", 0.3, 0.84],
            ["um", 0.9, 1.1],
            ["world.", 1.2, 1.8]
        ])
    );
    assert!(
        (v["duration_sec"].as_f64().unwrap() - 2.0).abs() < 0.01,
        "{v}"
    );
    let summary = v["summary"].as_str().unwrap();
    assert!(summary.contains("Pass track 1 to cut_words"), "{summary}");

    // The node is labelled, and records which model made it.
    let head = p.store.head().unwrap();
    let node = p.store.get(head).unwrap();
    assert_eq!(node.label.as_deref(), Some("transcribe track 1 (3 words)"));
    let op = node.op.expect("an op");
    assert_eq!(op.tool, "transcribe");
    assert!(!op.reproducible, "model output is not replayable");
    assert_eq!(op.params, json!({ "track": 1 }));
    assert_eq!(
        op.inputs["model"],
        json!({ "id": "whisper-base", "version": "abc123" })
    );
}

#[test]
fn a_file_is_transcribed_with_its_own_times_and_says_so() {
    let (_fake, mut p) = project();
    let other = p.dir.path().join("elsewhere.wav");
    speech::write_wav(&other, &tone(1.0, 440.0));

    let v = p.ok("transcribe", json!({ "path": other.to_string_lossy() }));

    let t = p.state().transcript.unwrap();
    assert_eq!(t.source, Some(TranscriptSource::File));
    let summary = v["summary"].as_str().unwrap();
    assert!(summary.contains("the file's own"), "{summary}");
    assert!(summary.contains("pass `track`"), "{summary}");
}

#[test]
fn a_second_transcript_replaces_the_first_and_says_so() {
    let (_fake, mut p) = project();
    p.ok("transcribe", json!({ "track": 0 }));
    let v = p.ok("transcribe", json!({ "track": 1 }));
    assert_eq!(v["replaced_earlier_transcript"], true);
    assert!(v["summary"]
        .as_str()
        .unwrap()
        .contains("replaced the session's earlier transcript"));
}

#[test]
fn no_speech_is_an_answer_and_not_a_node() {
    let fake = Arc::new(Fake::new(Vec::new()));
    let mut p = Project::new(Arc::clone(&fake) as Arc<dyn tools::tool::transcribe::SpeechToText>);
    p.load("a.wav", &tone(2.0, 220.0));
    let before = (p.node_count(), p.store.head());

    let v = p.ok("transcribe", json!({ "track": 0 }));

    assert_eq!(
        (p.node_count(), p.store.head()),
        before,
        "the session is unchanged"
    );
    assert!(p.state().transcript.is_none());
    assert_eq!(v["word_count"], 0);
    let summary = v["summary"].as_str().unwrap();
    assert!(
        summary.contains("No speech found") && summary.contains("the session is unchanged"),
        "{summary}"
    );
    assert!(summary.contains("nothing to retry"), "{summary}");
    // And asking again answers from the cache: nothing was heard, once.
    let again = p.ok("transcribe", json!({ "track": 0 }));
    assert_eq!(again["cached"], true);
    assert_eq!(fake.runs(), 1);
}

// ---------------------------------------------------------------------
// The cache
// ---------------------------------------------------------------------

#[test]
fn the_same_audio_asked_for_again_costs_no_run() {
    let (fake, mut p) = project();
    let first = p.ok("transcribe", json!({ "track": 0 }));
    assert_eq!(first["cached"], false);
    assert_eq!(fake.runs(), 1);
    assert_eq!(p.cache_files().len(), 1, "one entry in the inference cache");

    let second = p.ok("transcribe", json!({ "track": 0 }));
    assert_eq!(second["cached"], true);
    assert_eq!(fake.runs(), 1, "the recogniser did not run again");
    assert_eq!(second["words"], first["words"]);
    assert!(second["summary"]
        .as_str()
        .unwrap()
        .contains("from the cache"));
    // The request still makes a node: the user asked for a transcript.
    assert_eq!(p.state().transcript.unwrap().words.len(), 3);
}

#[test]
fn the_cache_is_the_audio_and_not_the_track() {
    let (fake, mut p) = project();
    // The same samples on a third track.
    p.load("c.wav", &tone(2.0, 220.0));
    p.ok("transcribe", json!({ "track": 0 }));
    let v = p.ok("transcribe", json!({ "track": 2 }));
    assert_eq!(v["cached"], true);
    assert_eq!(fake.runs(), 1);
    // ...with the source of the track that was asked about.
    let want = p.state().tracks[2].id.clone();
    assert_eq!(
        p.state().transcript.unwrap().source,
        Some(TranscriptSource::Track { track: want })
    );
}

#[test]
fn other_audio_a_other_language_or_another_model_runs_again() {
    let (fake, mut p) = project();
    p.ok("transcribe", json!({ "track": 0 }));
    assert_eq!(fake.runs(), 1);

    p.ok("transcribe", json!({ "track": 1 }));
    assert_eq!(fake.runs(), 2, "different samples");

    p.ok("transcribe", json!({ "track": 0, "language": "de" }));
    assert_eq!(
        fake.runs(),
        3,
        "a language was asked for that was not before"
    );
    p.ok("transcribe", json!({ "track": 0, "language": "de" }));
    assert_eq!(fake.runs(), 3, "...and then it is cached too");

    fake.use_model("model-two");
    p.ok("transcribe", json!({ "track": 0 }));
    assert_eq!(
        fake.runs(),
        4,
        "a model that is not the one that made the entry"
    );
}

#[test]
fn an_entry_that_cannot_be_read_is_made_again() {
    let (fake, mut p) = project();
    p.ok("transcribe", json!({ "track": 0 }));
    for file in p.cache_files() {
        std::fs::write(file, b"not json").unwrap();
    }
    let v = p.ok("transcribe", json!({ "track": 0 }));
    assert_eq!(v["cached"], false);
    assert_eq!(fake.runs(), 2);
    assert_eq!(v["word_count"], 3);
}

// ---------------------------------------------------------------------
// The language
// ---------------------------------------------------------------------

#[test]
fn the_language_reaches_the_recogniser_trimmed_and_lower_case() {
    let (fake, mut p) = project();
    let params = |p: &Project| {
        p.store
            .get(p.store.head().unwrap())
            .unwrap()
            .op
            .unwrap()
            .params
    };
    p.ok("transcribe", json!({ "track": 0, "language": " EN " }));
    assert_eq!(params(&p), json!({ "track": 0, "language": "en" }));
    p.ok("transcribe", json!({ "track": 1, "language": "" }));
    assert_eq!(
        params(&p),
        json!({ "track": 1 }),
        "blank is no language, and none is recorded"
    );
    p.ok("transcribe", json!({ "track": 0 }));
    let seen = fake.languages.lock().unwrap().clone();
    assert_eq!(seen, vec![Some("en".to_string()), None, None]);
}

#[test]
fn an_unknown_language_is_an_error_the_agent_can_fix() {
    let (fake, mut p) = project();
    *fake.fails_with.lock().unwrap() = Some(SpeechError::UnknownLanguage("xx".into()));
    let before = p.node_count();
    let msg = p.err("transcribe", json!({ "track": 0, "language": "xx" }));
    assert!(msg.contains("unknown language \"xx\""), "{msg}");
    assert!(msg.contains("ISO 639-1"), "{msg}");
    assert!(msg.contains("leave `language` out"), "{msg}");
    assert!(
        MARKERS.iter().all(|m| !msg.contains(m)),
        "not a setup card: {msg}"
    );
    assert_eq!(p.node_count(), before, "nothing appended");
    assert!(p.cache_files().is_empty(), "a failure is not cached");
}

// ---------------------------------------------------------------------
// What a failure says
// ---------------------------------------------------------------------

#[test]
fn each_setup_problem_is_answered_with_its_marker_and_changes_nothing() {
    let cases = [
        (SpeechError::ModelNotDownloaded, MARKER_MODEL_NOT_DOWNLOADED),
        (
            SpeechError::RuntimeUnavailable("no ONNX Runtime library was found".into()),
            MARKER_RUNTIME_UNAVAILABLE,
        ),
        (SpeechError::ModelDamaged, MARKER_MODEL_DAMAGED),
        (SpeechError::ModelPathInvalid, MARKER_MODEL_PATH_INVALID),
    ];
    for (error, marker) in cases {
        let (fake, mut p) = project();
        *fake.model.lock().unwrap() = Err(error.clone());
        let before = (p.node_count(), p.store.head());

        let msg = p.err("transcribe", json!({ "track": 0 }));

        assert!(msg.starts_with(marker), "{error:?}: {msg}");
        assert_eq!(fake.runs(), 0, "{error:?}: the model was never run");
        assert_eq!((p.node_count(), p.store.head()), before, "{error:?}");
        assert!(p.cache_files().is_empty());
    }
}

#[test]
fn a_model_that_fails_while_running_is_damaged_news_for_the_agent_not_a_card() {
    let (fake, mut p) = project();
    *fake.fails_with.lock().unwrap() = Some(SpeechError::Failed("the decoder gave up".into()));
    let msg = p.err("transcribe", json!({ "track": 0 }));
    assert_eq!(msg, "transcription failed: the decoder gave up");
}

#[test]
fn a_cancel_is_an_error_that_says_nothing_changed() {
    let (fake, mut p) = project();
    *fake.fails_with.lock().unwrap() = Some(SpeechError::Cancelled);
    let before = p.node_count();
    let msg = p.err("transcribe", json!({ "track": 0 }));
    assert!(msg.contains("cancelled by the user"), "{msg}");
    assert!(msg.contains("Nothing was changed"), "{msg}");
    assert_eq!(p.node_count(), before);
}

// ---------------------------------------------------------------------
// The arguments
// ---------------------------------------------------------------------

#[test]
fn the_audio_has_to_be_named_once() {
    let (fake, mut p) = project();
    let file = p.dir.path().join("a.wav");
    let both = p.err(
        "transcribe",
        json!({ "track": 0, "path": file.to_string_lossy() }),
    );
    assert!(both.contains("not both"), "{both}");
    let neither = p.err("transcribe", json!({}));
    assert!(
        neither.contains("`track`") && neither.contains("`path`"),
        "{neither}"
    );
    let missing = p.err("transcribe", json!({ "path": "/no/such/file.wav" }));
    assert!(missing.starts_with("file not found"), "{missing}");
    let range = p.err("transcribe", json!({ "track": 9 }));
    assert!(range.contains("out of range"), "{range}");
    assert_eq!(fake.runs(), 0);
}

#[test]
fn the_schema_is_validated_before_the_tool_runs() {
    let (_fake, mut p) = project();
    for bad in [
        json!({ "track": "one" }),
        json!({ "track": 0, "speed": 2 }),
        json!({ "track": 0, "language": 5 }),
    ] {
        let mut ctx = tools::ToolContext {
            store: &mut p.store,
            engine: &mut p.engine,
            user_message: "",
            clipboard: &mut p.clipboard,
            allowed_tools: None,
        };
        assert!(
            p.dispatcher
                .invoke("transcribe", bad.clone(), &mut ctx)
                .is_err(),
            "{bad}"
        );
    }
}

#[test]
fn a_session_with_nothing_loaded_says_so() {
    let fake = Arc::new(Fake::new(hello()));
    let mut p = Project::new(Arc::clone(&fake) as Arc<dyn tools::tool::transcribe::SpeechToText>);
    let msg = p.err("transcribe", json!({ "track": 0 }));
    assert!(msg.contains("no session loaded"), "{msg}");
    assert_eq!(fake.runs(), 0);
    let _ = model_ref("unused");
}

// ---------------------------------------------------------------------
// What the model is told about the schema
// ---------------------------------------------------------------------

#[test]
fn the_schema_says_what_the_tool_does_and_does_not_call_itself_unimplemented() {
    let d = tools::ToolDispatcher::default_dispatcher();
    let schemas = d.tool_schemas();
    let schema: &Value = schemas
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == "transcribe")
        .expect("registered");
    let desc = schema["description"].as_str().unwrap();
    assert!(!desc.to_lowercase().contains("not implemented"), "{desc}");
    assert!(
        desc.contains("Settings → Account"),
        "the user downloads the model: {desc}"
    );
    assert!(desc.contains("do not try to download"), "{desc}");
    assert!(desc.contains("cut_words"), "{desc}");
    let props = schema["input_schema"]["properties"].as_object().unwrap();
    for name in ["track", "path", "language"] {
        assert!(props.contains_key(name), "{name}");
    }
    assert!(
        schema["input_schema"].get("required").is_none(),
        "track or path: the tool says which is missing"
    );
}
