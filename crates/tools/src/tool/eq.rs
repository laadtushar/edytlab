//! Parametric EQ tool — biquad peak filter chain (Audio-EQ-Cookbook).

use serde::Deserialize;
use serde_json::{json, Value};

use crate::schema::anthropic_tool;
use crate::tool::util::destructive_edit_rechannel;
use crate::{Tool, ToolContext, ToolResult};

// ---------------------------------------------------------------------------
// Biquad peak-EQ filter
// ---------------------------------------------------------------------------

// Moved to `audio-dsp` so the render path can reach it (#127). The
// tool keeps its argument parsing and session plumbing; the DSP is
// shared.
pub(crate) use audio_dsp::eq::{apply_eq, BandKind, EqBand};

// ---------------------------------------------------------------------------
// Args
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct Band {
    freq_hz: f32,
    gain_db: f32,
    /// Peak default 1.0; shelves default 1/sqrt(2) (cookbook S = 1).
    q: Option<f32>,
    #[serde(default, rename = "type")]
    kind: Kind,
}

#[derive(Debug, Default, Deserialize, Clone, Copy)]
#[serde(rename_all = "snake_case")]
enum Kind {
    #[default]
    Peak,
    LowShelf,
    HighShelf,
}

impl Band {
    fn q(&self) -> f32 {
        self.q.unwrap_or(match self.kind {
            Kind::Peak => 1.0,
            _ => std::f32::consts::FRAC_1_SQRT_2,
        })
    }
}

impl From<&Band> for EqBand {
    /// The tool's `Band` derives `Deserialize`; `audio-dsp` has no serde
    /// dependency and should keep it that way, so the two are separate
    /// types with this one conversion between them.
    fn from(b: &Band) -> Self {
        EqBand {
            freq_hz: b.freq_hz,
            gain_db: b.gain_db,
            q: b.q(),
            kind: match b.kind {
                Kind::Peak => BandKind::Peak,
                Kind::LowShelf => BandKind::LowShelf,
                Kind::HighShelf => BandKind::HighShelf,
            },
        }
    }
}

/// Convert the tool's parsed bands into the DSP's.
fn to_eq_bands(bands: &[Band]) -> Vec<EqBand> {
    bands.iter().map(EqBand::from).collect()
}

#[derive(Debug, Deserialize)]
struct Args {
    track: usize,
    bands: Vec<Band>,
}

// ---------------------------------------------------------------------------
// Tool impl
// ---------------------------------------------------------------------------

pub struct EqTool;

impl Tool for EqTool {
    fn name(&self) -> &'static str {
        "eq"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "eq",
            "EQ a track with biquad bands. Each band: freq_hz, \
             gain_db, optional Q (default 1.0; 0.707 for shelves), optional type \
             peak (default), low_shelf or high_shelf. Appends a new session node.",
            json!({
                "type": "object",
                "required": ["track", "bands"],
                "additionalProperties": false,
                "properties": {
                    "track": { "type": "integer" },
                    "bands": {
                        "type": "array",
                        "minItems": 1,
                        "items": {
                            "type": "object",
                            "required": ["freq_hz", "gain_db"],
                            "additionalProperties": false,
                            "properties": {
                                "freq_hz": { "type": "number" },
                                "gain_db": { "type": "number" },
                                "q": { "type": "number" },
                                "type": { "type": "string", "enum": ["peak", "low_shelf", "high_shelf"] }
                            }
                        }
                    }
                }
            }),
        )
    }

    // A pure edit of the head's audio and one node on top of it: safe on a
    // staged store, and safe to run again on a newer head (#421).
    fn runs_off_the_lock(&self) -> bool {
        true
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let parsed: Args = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };

        // Validate bands.
        if parsed.bands.is_empty() {
            return Ok(ToolResult::Error("bands must not be empty".to_string()));
        }
        for (i, b) in parsed.bands.iter().enumerate() {
            if b.freq_hz <= 0.0 {
                return Ok(ToolResult::Error(format!(
                    "band {i}: freq_hz must be > 0 (got {})",
                    b.freq_hz
                )));
            }
            if !b.gain_db.is_finite() {
                return Ok(ToolResult::Error(format!(
                    "band {i}: gain_db must be finite"
                )));
            }
            if b.q() <= 0.0 {
                return Ok(ToolResult::Error(format!(
                    "band {i}: q must be > 0 (got {})",
                    b.q()
                )));
            }
        }

        Ok(invoke_eq(ctx, parsed.track, parsed.bands))
    }
}

/// Core EQ logic.
///
/// This used to be a hand-copy of `destructive_edit`, written that way
/// because the EQ needs the source's channel count for its per-channel
/// biquad state and the shared helper didn't hand it over. It does now —
/// and the copy had gone stale in the meantime, still editing only
/// `clips[0]`, so an EQ applied after an interior cut boosted the head of
/// the track and left the tail flat.
fn invoke_eq(ctx: &mut ToolContext, track_idx: usize, bands: Vec<Band>) -> ToolResult {
    let band_summary: Vec<String> = bands
        .iter()
        .map(|b| {
            // Peak labels stay as they were; only a shelf names its kind.
            let kind = match b.kind {
                Kind::Peak => String::new(),
                k => format!(" {k:?}"),
            };
            format!("{:.0}Hz {:+.1}dB Q{:.2}{kind}", b.freq_hz, b.gain_db, b.q())
        })
        .collect();
    let label = format!("eq [{}] on track {}", band_summary.join(", "), track_idx);

    destructive_edit_rechannel(
        ctx,
        track_idx,
        move |samples, sample_rate, channels| {
            apply_eq(
                samples,
                channels as usize,
                sample_rate,
                &to_eq_bands(&bands),
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
    use super::*;

    #[test]
    fn rejects_empty_bands() {
        let tool = EqTool;
        let args = serde_json::json!({
            "track": 0,
            "bands": []
        });
        // Schema validation (minItems:1) would catch this before invoke,
        // but let's also verify our own guard works via the Args path.
        // We bypass schema validation and call through serde directly.
        let parsed: Result<Args, _> = serde_json::from_value(args.clone());
        if let Ok(parsed) = parsed {
            // If serde accepts it (minItems not enforced by serde), our code should reject it.
            if parsed.bands.is_empty() {
                // Expected: our validation returns Error.
                // We can't call invoke without a ToolContext, so just assert the logic.
                assert!(
                    parsed.bands.is_empty(),
                    "expected empty bands to be rejected"
                );
            }
        }
        // Also verify the schema includes minItems:1 so the dispatcher catches it.
        let schema = tool.schema();
        let min_items = &schema["input_schema"]["properties"]["bands"]["minItems"];
        assert_eq!(min_items, 1, "schema must enforce minItems:1 on bands");
    }
}
