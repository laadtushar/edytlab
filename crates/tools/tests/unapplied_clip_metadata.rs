//! `align_to_beat` records metadata that nothing reads.
//!
//! `time_stretch` and `pitch_shift` used to be here too. They now apply
//! a phase vocoder to the samples, so the honest contract for them is
//! the ordinary one and their tests live with the other destructive
//! tools. `align_to_beat` still needs audio warped onto a grid, which
//! nothing does, so it keeps the warning and the flag.
//!
//! Their descriptions used to say the engine "applies it at render time in
//! M22+", and their results said "(applied at next render)". M22 is the
//! streaming engine, and it shipped. It does not read any of these three
//! fields, so the promise was overdue rather than pending — the agent was
//! reporting a change to the user that the audio never received.
//!
//! Implementing the DSP is a large piece of work. Telling the truth in the
//! meantime is not. These tests pin the honest contract, and they are
//! deliberately written so that whoever implements the real thing has to
//! come here and flip the flag — at which point the description and the
//! summary are right next to the assertion that sent them.

use std::path::{Path, PathBuf};

use hound::{SampleFormat, WavSpec, WavWriter};
use serde_json::{json, Value};
use tempfile::TempDir;
use tools::{ToolContext, ToolDispatcher, ToolResult};

const SAMPLE_RATE: u32 = 8_000;

fn ok(result: ToolResult) -> Value {
    match result {
        ToolResult::Ok(v) => v,
        ToolResult::Error(msg) => panic!("expected Ok, got Error({msg})"),
    }
}

fn write_tone(dir: &Path) -> PathBuf {
    let path = dir.join("in.wav");
    let spec = WavSpec {
        channels: 1,
        sample_rate: SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut writer = WavWriter::create(&path, spec).expect("wav writer");
    for n in 0..SAMPLE_RATE as usize {
        let t = n as f32 / SAMPLE_RATE as f32;
        let s = (2.0 * std::f32::consts::PI * 440.0 * t).sin() * 0.4;
        let q = (s * 32_768.0)
            .round()
            .clamp(i16::MIN as f32, i16::MAX as f32) as i16;
        writer.write_sample(q).unwrap();
    }
    writer.finalize().unwrap();
    path
}

/// Invoke one of the three recording tools and return its result.
fn record(tool: &str, args: Value) -> (Value, PathBuf, TempDir) {
    let tmp = TempDir::new().expect("tempdir");
    let mut store = session::Store::open(tmp.path()).expect("open store");
    let mut engine = audio_engine::Engine::new();
    let dispatcher = ToolDispatcher::default_dispatcher();
    let src = write_tone(tmp.path());
    let out = tmp.path().join("out.wav");

    let mut clipboard: Option<Vec<f32>> = None;
    let res = {
        let mut ctx = ToolContext {
            store: &mut store,
            engine: &mut engine,
            user_message: "",
            clipboard: &mut clipboard,
        };
        ok(dispatcher
            .invoke("load", json!({ "path": src.to_string_lossy() }), &mut ctx)
            .unwrap());
        let res = ok(dispatcher.invoke(tool, args, &mut ctx).unwrap());
        let node_id = res["node_id"].as_str().unwrap().to_string();
        ok(dispatcher
            .invoke(
                "render_final",
                json!({
                    "node_id": node_id,
                    "format": "wav",
                    "out_path": out.to_string_lossy(),
                }),
                &mut ctx,
            )
            .unwrap());
        res
    };
    (res, out, tmp)
}

/// Every one of the three must declare, in its result, that the render
/// does not apply it.
///
/// This is the field an agent can act on without parsing prose. When the
/// DSP lands, flipping it to `true` is the change — and the description
/// and summary sitting beside it get updated in the same edit.
#[test]
fn all_three_declare_that_the_render_does_not_apply_them() {
    let tool = "align_to_beat";
    let (res, _, _tmp) = record(tool, json!({ "track": 0, "beat_grid": [0.0, 0.5, 1.0] }));
    assert_eq!(
        res["applied_at_render"],
        json!(false),
        "{tool} must declare that its value is recorded but not rendered"
    );
    let summary = res["summary"].as_str().unwrap();
    assert!(
        !summary.contains("applied at next render"),
        "{tool} still tells the caller the render will apply it: {summary}"
    );
}

/// And the description a model reads before choosing the tool must say so
/// too — by then the result is too late to prevent a wrong plan.
#[test]
fn all_three_warn_in_the_schema_the_model_reads() {
    let dispatcher = ToolDispatcher::default_dispatcher();
    let schemas = dispatcher.tool_schemas();
    let schemas = schemas.as_array().expect("tool_schemas returns an array");

    let tool = "align_to_beat";
    let schema = schemas
        .iter()
        .find(|s| s["name"] == tool)
        .unwrap_or_else(|| panic!("{tool} is not registered"));
    let desc = schema["description"].as_str().unwrap();
    assert!(
        desc.contains("NOT YET APPLIED AT RENDER"),
        "{tool}'s description does not warn that it is inert: {desc}"
    );
    assert!(
        !desc.contains("M22+"),
        "{tool} still points at a milestone that has already shipped: {desc}"
    );
}

/// The two that were fixed must no longer carry the warning.
///
/// A stale "NOT YET APPLIED" on a tool that now works is the same
/// failure as the original, pointing the other way: the agent declines
/// to use something that would have done the job.
#[test]
fn the_implemented_tools_no_longer_warn() {
    let dispatcher = ToolDispatcher::default_dispatcher();
    let schemas = dispatcher.tool_schemas();
    let schemas = schemas.as_array().expect("tool_schemas returns an array");

    for tool in ["time_stretch", "pitch_shift"] {
        let schema = schemas
            .iter()
            .find(|s| s["name"] == tool)
            .unwrap_or_else(|| panic!("{tool} is not registered"));
        let desc = schema["description"].as_str().unwrap();
        assert!(
            !desc.contains("NOT YET APPLIED AT RENDER"),
            "{tool} works now; the warning would talk the agent out of using it"
        );
    }
}
