//! The pipeline on the real ONNX Runtime, with a model that is the pinned
//! htdemucs graph's inputs and outputs and none of its network
//! (`tests/fixtures/make_split_model.py`): every output is the input, a
//! quarter of it per source. What this holds in place, in CI's `ml.yml` and
//! on any machine with `ORT_DYLIB_PATH` set, is everything between the files
//! and the network that the fake in the unit tests cannot: the tensor shapes
//! and element types ONNX Runtime is given and gives back, the two-file
//! folder, the port check against a real session, and the session options.
//! It says nothing about separation; `real_model.rs` does that.
//!
//! All `#[ignore]`d with the reason, and *failing*, not skipping, when the
//! runtime is not there (the load answers `RuntimeUnavailable`).

use std::path::{Path, PathBuf};

use audio_decoder::DecodedAudio;
use ml_demucs::{Control, DemucsError, DemucsModel};
use ml_pipeline::onnx_session::SessionOptions;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn split_model() -> PathBuf {
    fixtures().join("htdemucs_split")
}

/// `seconds` of band-limited stereo at 44.1 kHz.
fn tones(seconds: f32) -> DecodedAudio {
    let frames = (seconds * 44_100.0) as usize;
    let mut samples = Vec::with_capacity(frames * 2);
    for i in 0..frames {
        let t = i as f32 / 44_100.0;
        let tau = 2.0 * std::f32::consts::PI;
        samples.push(0.3 * (tau * 220.0 * t).sin() + 0.1 * (tau * 3_300.0 * t).sin());
        samples.push(0.25 * (tau * 330.0 * t).sin() + 0.1 * (tau * 1_234.5 * t).sin());
    }
    DecodedAudio {
        samples,
        sample_rate: 44_100,
        channels: 2,
    }
}

fn read(path: &Path) -> Vec<f32> {
    hound::WavReader::open(path)
        .unwrap()
        .samples::<i16>()
        .map(|s| f32::from(s.unwrap()) / 32_768.0)
        .collect()
}

#[test]
#[ignore = "needs ORT_DYLIB_PATH (an ONNX Runtime library); run with --ignored"]
fn the_pipeline_on_a_real_session_splits_a_file_into_quarters() {
    let model = DemucsModel::load("htdemucs", &split_model()).expect("load the split model");
    let input = tones(9.0);
    let reference = input.samples.clone();
    let out = tempfile::tempdir().unwrap();
    let dest = out.path().join("entry");

    let seen = std::cell::RefCell::new(Vec::new());
    let progress = |d: usize, t: usize| seen.borrow_mut().push((d, t));
    let control = Control {
        cancel: &|| false,
        progress: &progress,
    };
    let done = model
        .separate_to_dir(input, &dest, &control)
        .expect("separate");

    // 9 s is 396,900 frames: two segments (one stride is 257,985).
    assert_eq!(done.segments, 2);
    assert_eq!(*seen.borrow(), [(0, 2), (1, 2), (2, 2)]);
    assert_eq!(
        (done.frames, done.sample_rate, done.channels),
        (396_900, 44_100, 2)
    );
    assert_eq!(done.clipped, [0; 4]);

    // Each stem is a quarter of the file, away from the two ends (where the
    // iSTFT is not exact: see spec.rs) and in the middle, where the
    // attenuated end of the first segment is blended with the second.
    let edge = 2048 * 2;
    for path in done.paths.iter() {
        let stem = read(path);
        assert_eq!(stem.len(), reference.len());
        let worst = (edge..stem.len() - edge)
            .map(|i| (stem[i] - 0.25 * reference[i]).abs())
            .fold(0.0f32, f32::max);
        assert!(worst < 2e-3, "{}: {worst}", path.display());
    }
    // And together they are the file.
    let sum: Vec<Vec<f32>> = done.paths.iter().map(read).collect();
    let worst = (edge..reference.len() - edge)
        .map(|i| (sum.iter().map(|s| s[i]).sum::<f32>() - reference[i]).abs())
        .fold(0.0f32, f32::max);
    assert!(worst < 4e-3, "the four stems add up to the file: {worst}");
}

#[test]
#[ignore = "needs ORT_DYLIB_PATH (an ONNX Runtime library); run with --ignored"]
fn the_session_options_reach_the_session() {
    // One thread and the lowest optimisation level: a session that was built
    // with them runs the same model to the same answer.
    let options = SessionOptions {
        intra_op_threads: Some(1),
        optimization_level: Some(ml_pipeline::GraphOptimizationLevel::Level1),
        ..SessionOptions::default()
    };
    let model = DemucsModel::load_with("htdemucs", &split_model(), options).expect("load");
    let out = tempfile::tempdir().unwrap();
    let done = model
        .separate_to_dir(tones(2.0), &out.path().join("entry"), &Control::none())
        .expect("separate");
    assert_eq!(done.segments, 1);
    let stem = read(&done.paths.vocals);
    assert!(stem.iter().any(|v| v.abs() > 0.01), "not silence");
}

#[test]
#[ignore = "needs ORT_DYLIB_PATH (an ONNX Runtime library); run with --ignored"]
fn a_cancel_on_the_real_session_leaves_no_entry() {
    let model = DemucsModel::load("htdemucs", &split_model()).expect("load");
    let out = tempfile::tempdir().unwrap();
    let dest = out.path().join("entry");
    let control = Control {
        cancel: &|| true,
        progress: &|_, _| {},
    };
    let err = model
        .separate_to_dir(tones(9.0), &dest, &control)
        .unwrap_err();
    assert!(matches!(err, DemucsError::Cancelled), "{err:?}");
    assert_eq!(std::fs::read_dir(out.path()).unwrap().count(), 0);
}

/// A graph that is not the pinned one is refused by name when the session is
/// built, not run: ml-pipeline's identity model has an input `X` and an
/// output `Y`.
#[test]
#[ignore = "needs ORT_DYLIB_PATH (an ONNX Runtime library); run with --ignored"]
fn a_different_graph_is_named_not_run() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::copy(
        fixtures().join("../../../ml-pipeline/tests/fixtures/identity.onnx"),
        dir.path().join("htdemucs_fwd.onnx"),
    )
    .unwrap();
    std::fs::write(dir.path().join("htdemucs_fwd.onnx.data"), b"").unwrap();
    let err = DemucsModel::load("htdemucs", dir.path()).unwrap_err();
    assert!(matches!(err, DemucsError::UnexpectedExport(_)), "{err:?}");
    assert!(err.to_string().contains("no `x`"), "{err}");
}

/// Both files are needed, and the folder not being there is not a crash.
#[test]
#[ignore = "needs ORT_DYLIB_PATH (an ONNX Runtime library); run with --ignored"]
fn a_folder_without_its_weights_or_not_there_is_model_missing() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::copy(
        split_model().join("htdemucs_fwd.onnx"),
        dir.path().join("htdemucs_fwd.onnx"),
    )
    .unwrap();
    let err = DemucsModel::load("htdemucs", dir.path()).unwrap_err();
    assert!(matches!(err, DemucsError::ModelMissing { .. }), "{err:?}");
    let err = DemucsModel::load("htdemucs", &dir.path().join("nope")).unwrap_err();
    assert!(matches!(err, DemucsError::ModelMissing { .. }), "{err:?}");
}
