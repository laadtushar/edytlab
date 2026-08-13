//! `align_to_beat` — warp a track so its beats land on a target grid.
//!
//! This was a metadata-only tool: it recorded a `beat_grid` on every
//! clip, nothing read it, and the result said `applied_at_render: false`
//! while the description told the model not to claim the audio had
//! changed. It was the last tool in the repo reporting success without
//! changing anything.
//!
//! It now warps. The DSP is `audio_time::warp_to_grid`, a single vocoder
//! pass with a synthesis hop that varies per frame — see that module for
//! why a stretch-per-segment would put a seam on every beat.

use serde::Deserialize;
use serde_json::{json, Value};

use crate::schema::anthropic_tool;
use crate::tool::util::destructive_edit_rechannel;
use crate::{Tool, ToolContext, ToolResult};

#[derive(Debug, Deserialize)]
struct Args {
    track: usize,
    /// Where the beats are now, in seconds. From `analyze_track`.
    source_beats: Vec<f32>,
    /// Where they should be, in seconds.
    beat_grid: Vec<f32>,
}

pub struct AlignToBeatTool;

/// Check one grid: finite, non-negative, strictly increasing.
fn validate(name: &str, grid: &[f32]) -> Result<(), String> {
    if grid.len() < 2 {
        return Err(format!(
            "{name} needs at least two beats to warp between; got {}",
            grid.len()
        ));
    }
    for (i, &t) in grid.iter().enumerate() {
        if !t.is_finite() || t < 0.0 {
            return Err(format!("{name}[{i}] = {t} must be finite and non-negative"));
        }
    }
    for i in 1..grid.len() {
        if grid[i] <= grid[i - 1] {
            return Err(format!(
                "{name} must be strictly increasing; got {} after {} at index {i}",
                grid[i],
                grid[i - 1],
            ));
        }
    }
    Ok(())
}

impl Tool for AlignToBeatTool {
    fn name(&self) -> &'static str {
        "align_to_beat"
    }

    fn schema(&self) -> Value {
        // Hand-rolled: `object_schema` doesn't compose array item types.
        let input_schema = json!({
            "type": "object",
            "properties": {
                "track": { "type": "integer" },
                "source_beats": {
                    "type": "array",
                    "items": { "type": "number" },
                    "minItems": 2,
                    "description": "Where the beats are now, in seconds from the start of the track. Get these from analyze_track.",
                },
                "beat_grid": {
                    "type": "array",
                    "items": { "type": "number" },
                    "minItems": 2,
                    "description": "Where those beats should end up, in seconds. Must have the same number of entries as source_beats.",
                },
            },
            "required": ["track", "source_beats", "beat_grid"],
            "additionalProperties": false,
        });
        anthropic_tool(
            "align_to_beat",
            "Warp a track in time so the beats at source_beats land on beat_grid, without changing \
             its pitch. Use it to fix drifting timing or to conform a performance to a click. Get \
             source_beats from analyze_track. The two arrays must be the same length — a mismatch \
             is an error rather than a partial warp. Each segment between consecutive beats is \
             stretched by its own ratio in a single pass, so there is no seam at the beats. \
             This rewrites the track's audio and appends a new session node.",
            input_schema,
        )
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };

        if let Err(msg) = validate("source_beats", &args.source_beats) {
            return Ok(ToolResult::Error(msg));
        }
        if let Err(msg) = validate("beat_grid", &args.beat_grid) {
            return Ok(ToolResult::Error(msg));
        }
        if args.source_beats.len() != args.beat_grid.len() {
            return Ok(ToolResult::Error(format!(
                "source_beats and beat_grid must have the same number of beats; got {} and {}. \
                 Truncating to the shorter would silently drop the rest of the arrangement",
                args.source_beats.len(),
                args.beat_grid.len()
            )));
        }

        let (track, beats) = (args.track, args.beat_grid.len());
        let (source_secs, target_secs) = (args.source_beats.clone(), args.beat_grid.clone());

        let mut failure: Option<String> = None;
        let result = destructive_edit_rechannel(
            ctx,
            track,
            |samples, sample_rate, channels| {
                // Seconds to samples happens here rather than in the
                // argument parsing, because the sample rate is not known
                // until the clip is decoded.
                let to_samples = |secs: &[f32]| -> Vec<u64> {
                    secs.iter()
                        .map(|t| (t * sample_rate as f32).round().max(0.0) as u64)
                        .collect()
                };
                match audio_time::warp_to_grid(
                    samples,
                    sample_rate,
                    channels,
                    &to_samples(&source_secs),
                    &to_samples(&target_secs),
                ) {
                    Ok(out) => *samples = out,
                    // The buffer is left untouched, so the edit writes
                    // the audio back unchanged and the caller gets the
                    // reason.
                    Err(e) => failure = Some(e.to_string()),
                }
                channels
            },
            format!("align_to_beat track {track} ({beats} beats)"),
        );

        if let Some(msg) = failure {
            return Ok(ToolResult::Error(msg));
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dispatcher::ToolDispatcher;

    #[test]
    fn schema_advertises_required_args() {
        let s = AlignToBeatTool.schema();
        assert_eq!(s["name"], "align_to_beat");
        let required = s["input_schema"]["required"].as_array().unwrap();
        let names: Vec<&str> = required.iter().map(|v| v.as_str().unwrap()).collect();
        assert!(names.contains(&"track"));
        assert!(names.contains(&"beat_grid"));
    }

    #[test]
    fn dispatcher_registers_align_to_beat() {
        let d = ToolDispatcher::default_dispatcher();
        assert!(d.get("align_to_beat").is_some());
    }
}
