//! Behavior tests for the public ONNX session builder.

use std::path::Path;

use ml_pipeline::onnx_session::{self, SessionOptions};
use ml_pipeline::{Error, GraphOptimizationLevel};

mod ort_support;

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
    let (shape, data) = ort_support::run_identity(&mut session, input);

    assert_eq!(shape, [1, 3], "Identity keeps the shape");
    assert_eq!(data, input, "Identity must return its input");
}
