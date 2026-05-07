//! Behaviour tests for the M07 tool dispatcher.
//!
//! No tools from M08/M09 exist yet, so we drive the dispatcher with a
//! tiny in-test fixture tool (`LoadTool`) that mirrors the eventual
//! `load` tool's argument shape but does no real work.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use serde_json::{json, Value};
use tempfile::TempDir;
use tools::{
    schema::{anthropic_tool, object_schema},
    DispatchError, Tool, ToolContext, ToolDispatcher, ToolResult,
};

/// Fixture tool that echoes its arguments back as the result.
struct LoadTool;

impl Tool for LoadTool {
    fn name(&self) -> &'static str {
        "load"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "load",
            "Load an audio file into the session as a new clip on a new track.",
            object_schema(&[("path", "string", true)]),
        )
    }

    fn invoke(&self, args: Value, _ctx: &mut ToolContext) -> tools::Result<ToolResult> {
        Ok(ToolResult::Ok(json!({ "echoed": args })))
    }
}

/// Fixture tool with the same schema as `LoadTool`, but flips a flag
/// when actually invoked. Used to assert that the dispatcher rejects
/// malformed args *before* reaching the tool.
struct RecordingTool {
    called: Arc<AtomicBool>,
}

impl Tool for RecordingTool {
    fn name(&self) -> &'static str {
        "load"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "load",
            "fixture",
            object_schema(&[("path", "string", true)]),
        )
    }

    fn invoke(&self, _args: Value, _ctx: &mut ToolContext) -> tools::Result<ToolResult> {
        self.called.store(true, Ordering::SeqCst);
        Ok(ToolResult::Ok(Value::Null))
    }
}

/// Build a `(Store, Engine)` pair backed by a fresh tempdir for each test.
fn fresh_state() -> (TempDir, session::Store, audio_engine::Engine) {
    let tmp = TempDir::new().expect("tempdir");
    let store = session::Store::open(tmp.path()).expect("open store");
    let engine = audio_engine::Engine::new();
    (tmp, store, engine)
}

#[test]
fn registering_and_invoking_a_noop_tool_returns_ok() {
    let mut dispatcher = ToolDispatcher::new();
    dispatcher.register(Box::new(LoadTool));

    let (_tmp, mut store, mut engine) = fresh_state();
    let mut ctx = ToolContext {
        store: &mut store,
        engine: &mut engine,
    };

    let out = dispatcher
        .invoke("load", json!({ "path": "/tmp/song.wav" }), &mut ctx)
        .expect("invoke succeeds");

    match out {
        ToolResult::Ok(v) => {
            assert_eq!(v["echoed"]["path"], "/tmp/song.wav");
        }
        ToolResult::Error(msg) => panic!("expected Ok, got Error({msg})"),
    }
}

#[test]
fn invoking_unregistered_tool_returns_unknown_with_name() {
    let dispatcher = ToolDispatcher::new();
    let (_tmp, mut store, mut engine) = fresh_state();
    let mut ctx = ToolContext {
        store: &mut store,
        engine: &mut engine,
    };

    let err = dispatcher
        .invoke("nope", json!({}), &mut ctx)
        .expect_err("should fail");

    match &err {
        DispatchError::Unknown(name) => assert_eq!(name, "nope"),
        other => panic!("expected Unknown, got {other:?}"),
    }
    // The Display impl must include the requested name so log output is
    // useful when an unknown tool is invoked.
    assert!(format!("{err}").contains("nope"));
}

#[test]
fn schema_validation_rejects_malformed_args_before_invoking() {
    let flag = Arc::new(AtomicBool::new(false));
    let mut dispatcher = ToolDispatcher::new();
    dispatcher.register(Box::new(RecordingTool {
        called: flag.clone(),
    }));

    let (_tmp, mut store, mut engine) = fresh_state();
    let mut ctx = ToolContext {
        store: &mut store,
        engine: &mut engine,
    };

    // `path` is missing — must be rejected.
    let err = dispatcher
        .invoke("load", json!({}), &mut ctx)
        .expect_err("should reject malformed args");
    assert!(matches!(err, DispatchError::SchemaValidation(_)));
    assert!(!flag.load(Ordering::SeqCst), "tool must not be invoked");

    // Wrong type for path — also rejected.
    let err = dispatcher
        .invoke("load", json!({ "path": 42 }), &mut ctx)
        .expect_err("should reject wrong type");
    assert!(matches!(err, DispatchError::SchemaValidation(_)));
    assert!(!flag.load(Ordering::SeqCst), "tool must not be invoked");

    // Valid args — tool runs.
    dispatcher
        .invoke("load", json!({ "path": "/x.wav" }), &mut ctx)
        .expect("valid args dispatch");
    assert!(flag.load(Ordering::SeqCst));
}

#[test]
fn tool_schemas_export_matches_anthropic_shape_snapshot() {
    let mut dispatcher = ToolDispatcher::new();
    dispatcher.register(Box::new(LoadTool));

    let exported = dispatcher.tool_schemas();
    let arr = exported.as_array().expect("tool_schemas returns an array");
    assert_eq!(arr.len(), 1);
    let actual = &arr[0];

    let snapshot_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/snapshots/anthropic_tool_schema_shape.json");
    let raw = std::fs::read_to_string(&snapshot_path).expect("read snapshot");
    let expected: Value = serde_json::from_str(&raw).expect("parse snapshot");

    assert_eq!(
        actual, &expected,
        "dispatcher schema export must match the checked-in Anthropic shape\n\
         expected: {expected:#}\n\
         actual:   {actual:#}",
    );
}
