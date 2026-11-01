//! What the tests that run a real ONNX Runtime share: running the committed
//! `identity.onnx`, and reading the runtime the app pins.
//!
//! A test binary includes this with `mod ort_support;` and uses a subset.
//! None of it needs a library until a function that builds a session is
//! called, so the tests that use it stay `#[ignore]`d.
#![allow(dead_code)]

use std::path::Path;

/// `ORT_DYLIB_PATH`, which an `#[ignore]`d test needs set.
pub fn ort_dylib_path() -> String {
    std::env::var("ORT_DYLIB_PATH").unwrap_or_else(|_| {
        panic!("ORT_DYLIB_PATH is not set; scripts/fetch-ml-runtime.sh <platform> <dir> fetches it")
    })
}

/// `apps/desktop/src-tauri/ml-runtime.json`: the library the installers carry.
pub fn pinned_runtime_manifest() -> serde_json::Value {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("apps")
        .join("desktop")
        .join("src-tauri")
        .join("ml-runtime.json");
    serde_json::from_str(&std::fs::read_to_string(&manifest).expect("read the manifest"))
        .expect("the manifest is JSON")
}

/// Run `identity.onnx` (`Y = X`, float32 `[1, 3]`) on `input` and return the
/// shape and the values that came out.
pub fn run_identity(session: &mut ort::session::Session, input: [f32; 3]) -> (Vec<i64>, Vec<f32>) {
    let tensor =
        ort::value::Tensor::from_array(([1usize, 3], input.to_vec())).expect("input tensor");
    let outputs = session
        .run(ort::inputs!["X" => tensor])
        .expect("run identity.onnx");
    let (shape, data) = outputs["Y"]
        .try_extract_tensor::<f32>()
        .expect("Y is a float tensor");
    (shape[..].to_vec(), data.to_vec())
}
