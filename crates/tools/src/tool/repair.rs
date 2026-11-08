//! Destructive repair tools; all output is written by the shared audio store.
use serde::Deserialize;
use serde_json::{json, Value};

use crate::schema::anthropic_tool;
use crate::tool::util::{
    append_state, check_seconds_order, destructive_edit_then, flatten_track, load_track_state,
    parse_args,
};
use crate::{Tool, ToolContext, ToolResult};

#[derive(Deserialize)]
struct DeclipArgs {
    track: usize,
    threshold_dbfs: Option<f32>,
}
pub struct DeclipTool;
impl Tool for DeclipTool {
    fn name(&self) -> &'static str {
        "declip"
    }
    fn schema(&self) -> Value {
        anthropic_tool("declip", "Reconstruct clipped peaks on a track with cubic interpolation. Set threshold_dbfs to the clipping level (default -0.01 dBFS, near full scale). Reports same-sign runs per channel, found and repaired; runs without clean edges are left alone. Unclipped audio is kept byte-identical. Restored peaks above full scale apply a shared gain (gain_db) to avoid re-clipping in PCM. Changed audio appends a new node.", json!({
            "type":"object", "properties": {
                "track":{"type":"integer"},
                "threshold_dbfs":{"type":"number","minimum":-60,"maximum":0,"default":-0.01}
            }, "required":["track"], "additionalProperties":false
        }))
    }
    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: DeclipArgs = match parse_args(args) {
            Ok(a) => a,
            Err(r) => return Ok(r),
        };
        let threshold = args.threshold_dbfs.unwrap_or(-0.01);
        if !threshold.is_finite() || !(-60.0..=0.0).contains(&threshold) {
            return Ok(ToolResult::Error(
                "threshold_dbfs must be between -60 and 0".into(),
            ));
        }
        let state = match load_track_state(ctx, args.track) {
            Ok(s) => s,
            Err(e) => return Ok(ToolResult::Error(e)),
        };
        if state.tracks[args.track].clips.is_empty() {
            return Ok(ToolResult::Error(
                "track has no clips; nothing to repair".into(),
            ));
        }
        let mut audio = match flatten_track(&state.tracks[args.track].clips) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(e)),
        };
        // One PCM quantization step can put a plateau just below the supplied level.
        let report = audio_dsp::repair::declip(
            &mut audio.window,
            audio.channels as usize,
            10.0f32.powf(threshold / 20.0) * 0.9999,
        );
        let label = format!("declip track {} threshold={threshold} dBFS", args.track);
        let extras = || {
            serde_json::Map::from_iter([
                ("runs_found".into(), json!(report.found)),
                ("runs_repaired".into(), json!(report.repaired)),
                ("gain_db".into(), json!(report.gain_db)),
            ])
        };
        if report.repaired == 0 {
            // Rewriting to the store's PCM format would quantize clean float input.
            return Ok(match append_state(ctx, state, &label) {
                Ok(id) => {
                    let mut value = extras();
                    value.insert("node_id".into(), json!(id.to_hex()));
                    value.insert("summary".into(), json!(label));
                    ToolResult::Ok(Value::Object(value))
                }
                Err(e) => ToolResult::Error(e),
            });
        }
        Ok(destructive_edit_then(
            ctx,
            args.track,
            move |samples, rate, channels| {
                *samples = audio.window;
                Ok((rate, channels))
            },
            |_, _| extras(),
            label,
        ))
    }
}

#[derive(Deserialize)]
struct SpanArgs {
    track: usize,
    start_sec: f64,
    end_sec: f64,
}
pub struct RepairSpanTool;
impl Tool for RepairSpanTool {
    fn name(&self) -> &'static str {
        "repair_span"
    }
    fn schema(&self) -> Value {
        anthropic_tool("repair_span", "Interpolate a damaged span of up to 100 ms on a track, in session seconds. Uses bidirectional tonal prediction, or bounded cubic interpolation when prediction is unstable. Needs two clean samples on each side. Preserves duration and channels and appends a new node.", json!({
            "type":"object", "properties": {
                "track":{"type":"integer"}, "start_sec":{"type":"number","minimum":0}, "end_sec":{"type":"number","minimum":0}
            }, "required":["track","start_sec","end_sec"], "additionalProperties":false
        }))
    }
    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: SpanArgs = match parse_args(args) {
            Ok(a) => a,
            Err(r) => return Ok(r),
        };
        if let Err(e) = check_seconds_order(args.start_sec, args.end_sec) {
            return Ok(ToolResult::Error(e));
        }
        if args.end_sec - args.start_sec > 0.1 + 1e-12 {
            return Ok(ToolResult::Error("repair_span is limited to 100 ms".into()));
        }
        Ok(destructive_edit_then(
            ctx,
            args.track,
            |samples, rate, channels| {
                let start = (args.start_sec * f64::from(rate)) as usize;
                let end = (args.end_sec * f64::from(rate)) as usize;
                // Validate before the DSP's index arithmetic, including huge arguments.
                let frames = samples.len() / channels as usize;
                if start < 2 || end >= frames.saturating_sub(1) || start >= end {
                    return Err(
                        "repair_span needs a nonempty range with two samples on each side".into(),
                    );
                }
                audio_dsp::repair::repair_span(samples, rate, channels as usize, start, end)
                    .map_err(str::to_owned)?;
                Ok((rate, channels))
            },
            |_, _| serde_json::Map::default(),
            format!(
                "repair_span track {} {}-{}s",
                args.track, args.start_sec, args.end_sec
            ),
        ))
    }
}
