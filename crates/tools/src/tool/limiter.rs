use crate::schema::anthropic_tool;
use crate::tool::util::{check_optional_seconds_order, destructive_edit_rechannel};
use crate::{Tool, ToolContext, ToolResult};
use audio_dsp::dynamics::{DEFAULT_RELEASE_MS, MAX_RELEASE_MS, MIN_RELEASE_MS};
use serde::Deserialize;
use serde_json::Value;

pub(crate) use audio_dsp::effects::limiter::apply_limiter;

#[derive(Debug, Deserialize)]
struct Args {
    track: usize,
    ceiling_db: f32,
    /// Omitted means [`DEFAULT_RELEASE_MS`]. Optional so that calls
    /// written before the parameter existed keep working.
    release_ms: Option<f32>,
    start_sec: Option<f64>,
    end_sec: Option<f64>,
}

pub struct LimiterTool;

impl Tool for LimiterTool {
    fn name(&self) -> &'static str {
        "limiter"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "limiter",
            "Peak limiter: turns the gain down just enough that no sample exceeds ceiling_db, then lets it recover smoothly over release_ms. Transients are tamed without the harmonic distortion of hard clipping, and all channels share one gain so the stereo image holds. Zero latency; the ceiling is a sample-peak ceiling, not an inter-sample true-peak one. Prevents digital clipping. Appends a new session node.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "track": { "type": "integer" },
                    "ceiling_db": { "type": "number", "description": "Maximum peak level in dBFS (e.g. -1.0)" },
                    "release_ms": { "type": "number", "minimum": MIN_RELEASE_MS, "maximum": MAX_RELEASE_MS, "description": format!("How long the gain takes to recover after a peak, in milliseconds. Default {DEFAULT_RELEASE_MS}. Shorter keeps the level up but distorts bass more; longer is smoother but holds the level down for longer after each peak.") },
                    "start_sec": { "type": "number" },
                    "end_sec": { "type": "number" }
                },
                "required": ["track", "ceiling_db"]
            }),
        )
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };

        // A reversed window would survive independent clamping and
        // panic on the slice below.
        if let Err(e) = check_optional_seconds_order(args.start_sec, args.end_sec) {
            return Ok(ToolResult::Error(e));
        }
        if args.ceiling_db > 0.0 {
            return Ok(ToolResult::Error("ceiling_db must be <= 0.0".into()));
        }
        // Rejected rather than clamped: a release of 0 asks for a hard
        // clip, and quietly substituting 1 ms would hide that the
        // request was not what the limiter does.
        let release_ms = args.release_ms.unwrap_or(DEFAULT_RELEASE_MS);
        if !release_ms.is_finite() || !(MIN_RELEASE_MS..=MAX_RELEASE_MS).contains(&release_ms) {
            return Ok(ToolResult::Error(format!(
                "release_ms must be between {MIN_RELEASE_MS} and {MAX_RELEASE_MS} (got {release_ms})"
            )));
        }

        let (ceiling, s, e) = (args.ceiling_db, args.start_sec, args.end_sec);
        // The channel count comes from the flattened track itself, not
        // from decoding the first clip's source: the gain is shared
        // across a frame's channels, so it has to be the count of the
        // buffer actually being limited.
        Ok(destructive_edit_rechannel(
            ctx,
            args.track,
            move |samples, sr, channels| {
                let ch = (channels as usize).max(1);
                let len_frames = samples.len() / ch;
                let start = s
                    .map(|sec| ((sec * sr as f64) as usize).min(len_frames))
                    .unwrap_or(0);
                let end = e
                    .map(|sec| ((sec * sr as f64) as usize).min(len_frames))
                    .unwrap_or(len_frames);
                apply_limiter(
                    &mut samples[start * ch..end * ch],
                    sr,
                    ch,
                    ceiling,
                    release_ms,
                );
                channels
            },
            format!(
                "limiter track {} ceiling={}dBFS release={}ms",
                args.track, ceiling, release_ms
            ),
        ))
    }
}
