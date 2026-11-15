//! `low_pass_filter` and `high_pass_filter` through the tool surface: both
//! share one `CutoffFilter`, so each is called the way the model calls it.

use serde_json::json;
use tools::tool::{HighPassFilterTool, LowPassFilterTool};
use tools::{Tool, ToolContext, ToolResult};

#[path = "support/harness.rs"]
mod harness;
use harness::Session;

const RATE: u32 = 48_000;

fn rms(pcm: &[i16]) -> f64 {
    let sum: f64 = pcm.iter().map(|&x| f64::from(x).powi(2)).sum();
    (sum / pcm.len().max(1) as f64).sqrt()
}

/// One second of a single tone at `freq`.
fn tone(freq: f32) -> Session {
    Session::with_tones(RATE, RATE as usize, &[(freq, 0.5)])
}

#[test]
fn a_low_pass_removes_a_tone_above_its_cutoff() {
    let mut s = tone(9_000.0);
    let before = rms(&s.render_pcm(None));
    s.ok("low_pass_filter", json!({ "track": 0, "cutoff_hz": 500.0 }));
    let after = rms(&s.render_pcm(None));
    assert!(after < before * 0.1, "before {before}, after {after}");
}

#[test]
fn a_high_pass_removes_a_tone_below_its_cutoff() {
    let mut s = tone(100.0);
    let before = rms(&s.render_pcm(None));
    s.ok(
        "high_pass_filter",
        json!({ "track": 0, "cutoff_hz": 5_000.0 }),
    );
    let after = rms(&s.render_pcm(None));
    assert!(after < before * 0.1, "before {before}, after {after}");
}

#[test]
fn a_cutoff_that_is_not_positive_is_refused() {
    for tool in ["low_pass_filter", "high_pass_filter"] {
        for cutoff in [0.0, -10.0] {
            let mut s = tone(1_000.0);
            assert_eq!(
                s.err(tool, json!({ "track": 0, "cutoff_hz": cutoff })),
                "cutoff_hz must be positive",
                "{tool} {cutoff}"
            );
        }
    }
}

#[test]
fn a_call_without_a_cutoff_is_refused_before_it_runs() {
    for tool in ["low_pass_filter", "high_pass_filter"] {
        let mut s = tone(1_000.0);
        let message = s
            .try_call(tool, json!({ "track": 0 }))
            .expect_err("the dispatcher checks the schema")
            .to_string();
        assert!(message.contains("cutoff_hz"), "{tool}: {message}");
    }
}

#[test]
fn a_tool_called_directly_with_bad_arguments_answers_with_the_parse_error() {
    // The dispatcher refuses these first; the tool's own parse is the
    // backstop for a caller that skips the schema check.
    let tools: [&dyn Tool; 2] = [&LowPassFilterTool, &HighPassFilterTool];
    for tool in tools {
        let mut s = tone(1_000.0);
        let mut ctx = ToolContext {
            store: &mut s.store,
            engine: &mut s.engine,
            user_message: "",
            clipboard: &mut s.clipboard,
            allowed_tools: None,
        };
        match tool.invoke(json!({ "track": 0 }), &mut ctx).unwrap() {
            ToolResult::Error(m) => assert!(m.contains("cutoff_hz"), "{}: {m}", tool.name()),
            ToolResult::Ok(v) => panic!("{}: expected Error, got Ok({v})", tool.name()),
        }
    }
}

#[test]
fn both_filters_run_off_the_lock_under_their_own_names() {
    let low = LowPassFilterTool;
    let high = HighPassFilterTool;
    assert_eq!(low.name(), "low_pass_filter");
    assert_eq!(high.name(), "high_pass_filter");
    assert!(low.runs_off_the_lock());
    assert!(high.runs_off_the_lock());
}

#[test]
fn both_schemas_require_a_track_and_a_cutoff_and_take_an_optional_window() {
    let tools: [&dyn Tool; 2] = [&LowPassFilterTool, &HighPassFilterTool];
    for tool in tools {
        let schema = tool.schema();
        assert_eq!(schema["name"], tool.name());
        assert!(schema["description"]
            .as_str()
            .is_some_and(|d| !d.is_empty()));
        assert_eq!(
            schema["input_schema"]["required"],
            json!(["track", "cutoff_hz"]),
            "{}",
            tool.name()
        );
        for key in ["track", "cutoff_hz", "start_sec", "end_sec"] {
            assert!(
                schema["input_schema"]["properties"][key].is_object(),
                "{}: {key}",
                tool.name()
            );
        }
    }
}
