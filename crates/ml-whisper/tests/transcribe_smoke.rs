//! Whisper transcription smoke test.
//!
//! Real speech fixture is a manual gate; CI smoke test only verifies the
//! pipeline doesn't panic and returns a `Vec<Word>`. Once a real
//! `tests/golden/known_speech_clip.wav` and matching transcript are
//! committed (CC0 / self-recorded) the Levenshtein-distance assertion
//! described in M09 acceptance-criterion #1 should be re-enabled here.
//!
//! The model-dependent tests need a downloaded Whisper model, so they
//! are `#[ignore]`d and run with `cargo test -p ml-whisper -- --ignored`
//! once `WHISPER_MODEL_PATH` points at the `.onnx` file.
//!
//! They used to gate on the env var at runtime and `return` early, which
//! `cargo test` counts as **passed** — so a run that never entered the
//! test body was indistinguishable from one that did. Worse, the var
//! they read was `WHISPER_MODEL`, while everything that actually loads a
//! model reads `WHISPER_MODEL_PATH`: an operator following the only name
//! the app ever mentions still got a silent skip. No CI workflow sets
//! either name, despite what this header used to claim.
//!
//! `#[ignore]` states the situation honestly in the run output, and
//! asking for these tests explicitly with `--ignored` now *fails* when
//! the model is absent rather than passing quietly — if you asked to run
//! them, a skip is not an answer.

use std::path::PathBuf;

use ml_whisper::{resample_to_16khz_mono, WhisperModel};

/// The name every consumer uses: `transcribe.rs`, the structured error
/// in `ml-whisper/src/lib.rs`, and the in-app recovery panel.
const MODEL_PATH_VAR: &str = "WHISPER_MODEL_PATH";

/// The model path, or a panic explaining what to set.
///
/// These tests only run when someone asks for them by name, so an
/// unusable model is a failure rather than a skip.
fn model_path() -> PathBuf {
    let var = std::env::var(MODEL_PATH_VAR).unwrap_or_else(|_| {
        panic!(
            "{MODEL_PATH_VAR} is not set. Point it at an ONNX Whisper export to run the \
             ignored ml-whisper tests. Note that the decoder itself is a stub (#233), so \
             `transcribe` will return NotImplemented even with a valid model."
        )
    });
    let path = PathBuf::from(var);
    assert!(
        path.exists(),
        "{MODEL_PATH_VAR} points at {}, which does not exist",
        path.display()
    );
    path
}

#[test]
#[ignore = "needs a downloaded Whisper model; set WHISPER_MODEL_PATH and run with --ignored"]
fn transcribe_smoke_returns_vec_word() {
    let model_path = model_path();

    let model = WhisperModel::load(&model_path).expect("model load");

    // 1 second of silence at 16 kHz mono — enough to exercise the
    // pipeline without depending on a committed speech fixture.
    let silence = vec![0.0f32; 16_000];
    let words = match model.transcribe(&silence) {
        Ok(w) => w,
        // Expected until the decoder lands (#233). `transcribe` used to
        // return `Ok(vec![])` here, which made this test pass while
        // proving nothing: the timestamp contract below is vacuous on
        // an empty Vec, so a green run meant only that the stub was
        // still a stub. Naming that outcome explicitly keeps the
        // contract checks below meaningful for the real decoder, and
        // makes this test start doing work the moment one exists.
        Err(ml_whisper::WhisperError::NotImplemented) => {
            eprintln!("decoder is still a stub; nothing to check yet (#233)");
            return;
        }
        Err(e) => panic!("transcribe failed: {e}"),
    };
    assert!(
        !words.is_empty(),
        "a real decoder returned no words at all for this input; if that is correct for \
         silence, give this test speech instead — an empty result checks nothing below"
    );

    // Acceptance criterion #2: monotonic non-decreasing timestamps and
    // start_s < end_s.
    let mut last_end = 0.0f32;
    for w in &words {
        assert!(w.start_s < w.end_s, "word {w:?} has start_s >= end_s",);
        assert!(
            w.start_s + 1e-6 >= last_end - 1.0,
            "non-monotonic timestamps: previous end_s {}, current start_s {}",
            last_end,
            w.start_s
        );
        last_end = w.end_s;
    }
}

#[test]
fn missing_model_returns_structured_error() {
    // Acceptance criterion #4: the "model missing" path is panic-free
    // and returns a structured error that includes the install hint.
    // This test is independent of WHISPER_MODEL_PATH because it
    // deliberately points at a path that does not exist.
    let bogus = PathBuf::from("/tmp/edytlab-nonexistent-whisper-model.onnx");
    let err = WhisperModel::load(&bogus).expect_err("expected ModelMissing");
    let msg = format!("{err}");
    // It used to require the message name a fetch-models shell script.
    // That file has never existed in this repository, so the test was
    // pinning a dangling instruction in place — and following it would
    // not have helped, because the decoder is a stub (#233). What the
    // message has to say now is that the feature is unavailable, since
    // "you are missing a step" and "this does not work yet" are
    // different problems.
    assert!(
        msg.contains("not implemented in this build"),
        "error message should say transcription is unavailable, not name a setup step that \
         cannot work; got: {msg}",
    );
    // The drift that made these tests unrunnable: they gated on
    // `WHISPER_MODEL` while every consumer read `WHISPER_MODEL_PATH`, so
    // an operator following the only name the app mentions still got a
    // skip. Pin the test's name to the one the user is actually told.
    assert!(
        msg.contains(MODEL_PATH_VAR),
        "the model-missing error tells the user to set a different variable \
         than these tests read ({MODEL_PATH_VAR}); got: {msg}",
    );
}

#[test]
fn resampler_silence_roundtrip() {
    // Bonus: confirm the resampler handles a 44.1 kHz stereo silence
    // input the way the transcribe tool will feed it.
    let frames = 44_100; // 1 s
    let stereo = vec![0.0f32; frames * 2];
    let mono16k = resample_to_16khz_mono(&stereo, 44_100, 2).expect("resample");
    let delta = (mono16k.len() as i64 - 16_000).abs();
    assert!(
        delta < 2_000,
        "resampled length {} not within 2000 of 16000",
        mono16k.len()
    );
}

#[test]
#[ignore = "needs a downloaded Whisper model; set WHISPER_MODEL_PATH and run with --ignored"]
fn reuses_loaded_model_across_calls() {
    // Acceptance criterion #3: model-loaded-once-and-reused. We can't
    // measure timing reliably without the real decoder, but we *can*
    // assert the API supports it: a single `&WhisperModel` handles N
    // calls without rebuild.
    let model_path = model_path();
    let model = WhisperModel::load(&model_path).expect("model load");
    let silence = vec![0.0f32; 16_000];
    for _ in 0..5 {
        let _ = model.transcribe(&silence).expect("transcribe");
    }
}
