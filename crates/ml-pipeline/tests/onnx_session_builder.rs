//! Behavior tests for the public ONNX session builder.

use std::path::Path;

use ml_pipeline::onnx_session::{self, SessionOptions};
use ml_pipeline::{Error, GraphOptimizationLevel};

#[test]
fn missing_model_is_reported_before_runtime_lookup() {
    let err = onnx_session::build(
        Path::new("/definitely/does/not/exist.onnx"),
        SessionOptions::default(),
    )
    .expect_err("missing model path should be rejected");

    match err {
        Error::Io(error) => assert_eq!(error.kind(), std::io::ErrorKind::NotFound),
        other => panic!("expected Io(NotFound), got {other:?}"),
    }
}

#[test]
#[ignore = "needs ORT_DYLIB_PATH (an ONNX Runtime library); run with --ignored"]
fn configured_builder_runs_the_identity_model() {
    let model_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("identity.onnx");
    let mut session = onnx_session::build(
        &model_path,
        SessionOptions {
            intra_op_threads: Some(1),
            optimization_level: Some(GraphOptimizationLevel::Level1),
            ..SessionOptions::default()
        },
    )
    .expect("build identity session");

    let input = [1.0f32, -2.5, 0.0];
    let tensor =
        ort::value::Tensor::from_array(([1usize, 3], input.to_vec())).expect("input tensor");
    let outputs = session
        .run(ort::inputs!["X" => tensor])
        .expect("run identity.onnx");
    let (shape, data) = outputs["Y"]
        .try_extract_tensor::<f32>()
        .expect("Y is a float tensor");

    assert_eq!(&shape[..], &[1, 3], "Identity keeps the shape");
    assert_eq!(data, &input[..], "Identity must return its input");
}

/// The memory options reach the session: with the CPU arena and the memory
/// pattern both off (what a caller whose model needs gigabytes of working
/// memory asks for, #385), the identity model still builds and runs, and the
/// default still leaves both alone.
#[test]
#[ignore = "needs ORT_DYLIB_PATH (an ONNX Runtime library); run with --ignored"]
fn memory_options_build_a_session_that_runs() {
    let model_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("identity.onnx");
    let defaults = SessionOptions::default();
    assert_eq!(
        (defaults.cpu_memory_arena, defaults.memory_pattern),
        (None, None),
        "unset memory options keep ONNX Runtime's defaults"
    );
    for (arena, pattern) in [
        (Some(false), Some(false)),
        (Some(true), None),
        (None, Some(false)),
    ] {
        let mut session = onnx_session::build(
            &model_path,
            SessionOptions {
                cpu_memory_arena: arena,
                memory_pattern: pattern,
                ..SessionOptions::default()
            },
        )
        .unwrap_or_else(|e| panic!("arena {arena:?}, pattern {pattern:?}: {e}"));
        let tensor = ort::value::Tensor::from_array(([1usize, 3], vec![1.0f32, 2.0, 3.0])).unwrap();
        let outputs = session.run(ort::inputs!["X" => tensor]).unwrap();
        let (_, data) = outputs["Y"].try_extract_tensor::<f32>().unwrap();
        assert_eq!(data, &[1.0, 2.0, 3.0]);
    }
}
