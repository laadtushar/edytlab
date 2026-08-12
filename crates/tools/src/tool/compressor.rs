//! Dynamic compressor tool — envelope follower with threshold/ratio/attack/release.

use serde::Deserialize;
use serde_json::{json, Value};

use crate::schema::anthropic_tool;
use crate::tool::util::destructive_edit_rechannel;
use crate::{Tool, ToolContext, ToolResult};

// ---------------------------------------------------------------------------
// Core compression algorithm
// ---------------------------------------------------------------------------

// Moved to `audio-dsp` (#127) so the render path can use it.
pub(crate) use audio_dsp::compressor::compress_samples;

// ---------------------------------------------------------------------------
// Args
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct Args {
    track: usize,
    threshold_db: f32,
    ratio: f32,
    attack_ms: f32,
    release_ms: f32,
    #[serde(default)]
    makeup_db: f32,
}

// ---------------------------------------------------------------------------
// Tool impl
// ---------------------------------------------------------------------------

pub struct CompressorTool;

impl Tool for CompressorTool {
    fn name(&self) -> &'static str {
        "compressor"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "compressor",
            "Apply dynamic compression to a track using an envelope follower. \
             Reduces the gain of loud passages above a threshold by a given ratio. \
             Supports configurable attack/release times and optional makeup gain. \
             Appends a new session node.",
            json!({
                "type": "object",
                "required": ["track", "threshold_db", "ratio", "attack_ms", "release_ms"],
                "additionalProperties": false,
                "properties": {
                    "track": { "type": "integer" },
                    "threshold_db": { "type": "number" },
                    "ratio": { "type": "number" },
                    "attack_ms": { "type": "number" },
                    "release_ms": { "type": "number" },
                    "makeup_db": { "type": "number" }
                }
            }),
        )
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let parsed: Args = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };

        // Validate parameters.
        if parsed.ratio < 1.0 {
            return Ok(ToolResult::Error(format!(
                "ratio must be >= 1.0 (got {})",
                parsed.ratio
            )));
        }
        if !parsed.attack_ms.is_finite() || parsed.attack_ms <= 0.0 {
            return Ok(ToolResult::Error(format!(
                "attack_ms must be > 0 and finite (got {})",
                parsed.attack_ms
            )));
        }
        if !parsed.release_ms.is_finite() || parsed.release_ms <= 0.0 {
            return Ok(ToolResult::Error(format!(
                "release_ms must be > 0 and finite (got {})",
                parsed.release_ms
            )));
        }
        if !parsed.threshold_db.is_finite() {
            return Ok(ToolResult::Error(format!(
                "threshold_db must be finite (got {})",
                parsed.threshold_db
            )));
        }

        Ok(invoke_compressor(
            ctx,
            parsed.track,
            parsed.threshold_db,
            parsed.ratio,
            parsed.attack_ms,
            parsed.release_ms,
            parsed.makeup_db,
        ))
    }
}

/// Core compressor logic — follows the destructive_edit pattern from eq.rs.
/// Core compressor logic.
///
/// Formerly a hand-copy of `destructive_edit`, made because the envelope
/// follower needs the channel count for its per-frame peak and the shared
/// helper didn't provide it. It does now, and the copy had drifted — it
/// still edited only `clips[0]`, so compressing a track that an interior
/// cut had split left the tail uncompressed.
fn invoke_compressor(
    ctx: &mut ToolContext,
    track_idx: usize,
    threshold_db: f32,
    ratio: f32,
    attack_ms: f32,
    release_ms: f32,
    makeup_db: f32,
) -> ToolResult {
    let label = format!(
        "compressor [thresh={threshold_db:.1}dB ratio={ratio:.1}:1 \
         atk={attack_ms:.1}ms rel={release_ms:.1}ms makeup={makeup_db:.1}dB] on track {track_idx}"
    );

    destructive_edit_rechannel(
        ctx,
        track_idx,
        move |samples, sample_rate, channels| {
            *samples = compress_samples(
                samples,
                channels as usize,
                threshold_db,
                ratio,
                attack_ms,
                release_ms,
                makeup_db,
                sample_rate,
            );
            channels
        },
        label,
    )
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    // No `use super::*`: the DSP tests moved to `audio-dsp` with the
    // algorithm (#127), and what is left does not touch this module.
    //
    // Which is itself telling — the test below re-implements the ratio
    // check rather than calling the tool, so it asserts that `0.5 < 1.0`
    // and nothing about `CompressorTool`. Left as found; replacing it
    // with a real dispatcher test is worth doing but is not part of a
    // move.

    #[test]
    fn rejects_ratio_below_one() {
        // Validate the ratio check directly (no ToolContext needed).
        // ratio = 0.5 is invalid — compressor must expand, not compress.
        let ratio: f32 = 0.5;
        assert!(
            ratio < 1.0,
            "ratio {ratio} should be rejected (must be >= 1.0)"
        );
        // Verify the tool's invoke() would return an Error for this input.
        // We test the validation logic as it would be reached in invoke().
        let validation_result: Result<(), String> = if ratio < 1.0 {
            Err(format!("ratio must be >= 1.0 (got {ratio})"))
        } else {
            Ok(())
        };
        assert!(
            validation_result.is_err(),
            "expected validation to reject ratio < 1.0"
        );
    }
}
