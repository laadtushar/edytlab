//! `paulstretch` — stretch a track, or a range of it, to many times its
//! length as a smooth ambient texture.
//!
//! The DSP is `audio_time::paulstretch` (Paul Nasca's algorithm: keep each
//! window's magnitudes, draw its phases at random, overlap-add with a hop
//! that moves slower through the source). This tool applies it to the
//! track's samples and writes the result the way every other destructive
//! tool does: a new content-addressed WAV under the project's `derived/`
//! folder and a new session node, so undo works. It takes no path from the
//! caller, so there is no destination for `destination::refuse_project_audio`
//! to check: the only file written is the derived WAV, never a source.
//!
//! The phases are random, so the seed is part of the edit: an omitted `seed`
//! is [`DEFAULT_SEED`], never the clock, and a node replayed from its
//! recorded arguments (`rederive`) writes the same audio.
//!
//! With sync-lock on, a range stretch opens the gained time on every other
//! track, as `insert_silence` does (one node, `time_edit::apply`); a
//! whole-track stretch re-times one recording and, like `time_stretch`, moves
//! no other track.
//!
//! What moves with the audio. A whole-track stretch scales every mark and
//! word by the factor, as `time_stretch` does. A range is replaced by its
//! stretched self, so what lies after it moves later by what the range
//! gained; marks inside it stay at their times (the texture has no events
//! for them to point at), as they do for any edit that rewrites a span.

use std::cell::Cell;
use std::rc::Rc;

use serde::Deserialize;
use serde_json::Value;

use crate::schema::anthropic_tool;
use crate::tool::time_edit::{apply, Clips, Scope, SyncLock, TimeEdit, Words};
use crate::tool::util::{destructive_edit_then, load_head_state, parse_args};
use crate::util::range_resolver::resolve as resolve_range;
use crate::{Range, Tool, ToolContext, ToolResult};

/// The seed used when the call names none.
pub const DEFAULT_SEED: u64 = 0x5041_554C; // "PAUL"

/// The default analysis window, in seconds (the reference implementation's).
const DEFAULT_WINDOW_SEC: f32 = 0.25;

/// The longest result, in seconds. A stereo 48 kHz result this long is
/// about 460 MB of samples in memory while it is made, which is what a
/// desktop session can hold beside the rest of the project.
const MAX_OUTPUT_SEC: f64 = 1200.0;

#[derive(Debug, Deserialize)]
struct Args {
    track: usize,
    factor: f32,
    window_sec: Option<f32>,
    seed: Option<u64>,
    range: Option<Range>,
}

/// What the edit did, handed from the closure that ran it to the hook that
/// reports it: the span stretched and the size of the result, in seconds.
#[derive(Clone, Copy, Default)]
struct Done {
    start_sec: f64,
    end_sec: f64,
    window_sec: f64,
    output_sec: f64,
}

/// The `paulstretch` agent tool: Paul Nasca's extreme stretch of a track or a
/// range, written as one new session node.
pub struct PaulstretchTool;

impl Tool for PaulstretchTool {
    fn name(&self) -> &'static str {
        "paulstretch"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "paulstretch",
            "Paulstretch: stretch a track or `range` to `factor` times its length as a smooth ambient pad. Keeps the pitch, drops the attacks. factor 1 to 50; window_sec 0.02 to 5 (default 0.25; longer is smoother); same seed, same sound. Result at most 1200 s. With sync-lock on, a range opens the added time on every track. Appends a new session node.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "track": { "type": "integer" },
                    "factor": { "type": "number" },
                    "window_sec": { "type": "number" },
                    "seed": { "type": "integer" },
                    "range": {
                        "type": "object",
                        "properties": {
                            "start_sec": { "type": "number" },
                            "end_sec": { "type": "number" }
                        },
                        "required": ["start_sec", "end_sec"]
                    }
                },
                "required": ["track", "factor"]
            }),
        )
    }

    // A pure edit of the head's audio and one node on top of it: safe on a
    // staged store, and safe to run again on a newer head (#421).
    fn runs_off_the_lock(&self) -> bool {
        true
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match parse_args(args) {
            Ok(a) => a,
            Err(r) => return Ok(r),
        };
        let range = match resolve_range(args.range, ctx.user_message, false) {
            Ok(r) => r,
            Err(e) => return Ok(ToolResult::Error(e.to_string())),
        };
        let (track, factor) = (args.track, args.factor);
        let window_sec = args.window_sec.unwrap_or(DEFAULT_WINDOW_SEC);
        // Before any arithmetic on them: a factor of 1e20 saturates `as usize`.
        if !(audio_time::MIN_PAULSTRETCH_FACTOR..=audio_time::MAX_PAULSTRETCH_FACTOR)
            .contains(&factor)
        {
            return Ok(ToolResult::Error(format!(
                "invalid stretch factor: {factor} (must be finite and within {} to {})",
                audio_time::MIN_PAULSTRETCH_FACTOR,
                audio_time::MAX_PAULSTRETCH_FACTOR
            )));
        }
        if !(audio_time::MIN_PAULSTRETCH_WINDOW_SEC..=audio_time::MAX_PAULSTRETCH_WINDOW_SEC)
            .contains(&window_sec)
        {
            return Ok(ToolResult::Error(format!(
                "invalid window_sec: {window_sec} (must be finite and within {} to {} seconds)",
                audio_time::MIN_PAULSTRETCH_WINDOW_SEC,
                audio_time::MAX_PAULSTRETCH_WINDOW_SEC
            )));
        }
        let seed = args.seed.unwrap_or(DEFAULT_SEED);

        // Sync-lock needs each other track's own rate (#234, #673), asked before
        // anything is edited so a track with no single rate refuses the call.
        // Asked as a tool that follows sync-lock would, whether or not this
        // stretch carries anything to the other tracks: a track with no
        // single rate refuses a whole-track stretch too, as it always did.
        // Only a range carries (below).
        let asked = Scope::new(track, SyncLock::Follows, Clips::Rewritten, Words::Follow);
        let rates = match load_head_state(ctx).and_then(|state| asked.rates(&state)) {
            Ok(r) => r,
            Err(e) => return Ok(ToolResult::Error(e)),
        };
        let sync_lock = if range.is_some() {
            SyncLock::Follows
        } else {
            SyncLock::Ignores
        };
        let scope = Scope::new(track, sync_lock, Clips::Rewritten, Words::Follow);

        let done = Rc::new(Cell::new(Done::default()));
        let edit_done = Rc::clone(&done);
        let after_done = Rc::clone(&done);

        let span = match range {
            Some(r) => format!(" {:.2}s\u{2013}{:.2}s", r.start_sec, r.end_sec),
            None => String::new(),
        };
        let label = format!(
            "paulstretch track {track}{span} factor {factor:.2} window {window_sec:.3}s seed {seed}"
        );

        Ok(destructive_edit_then(
            ctx,
            track,
            move |samples, sample_rate, channels| {
                let stride = channels.max(1) as usize;
                let rate = sample_rate.max(1) as f64;
                let frames = samples.len() / stride;
                let (start, end) = match range {
                    Some(r) => (
                        ((r.start_sec * rate).round() as usize).min(frames),
                        ((r.end_sec * rate).round() as usize).min(frames),
                    ),
                    None => (0, frames),
                };
                if start >= end {
                    return Err(format!(
                        "range starts at {:.2}s, at or after the end of the track ({:.2}s)",
                        start as f64 / rate,
                        frames as f64 / rate
                    ));
                }
                let span = end - start;
                let stretched_frames = (span as f64 * factor as f64).round() as usize;
                let result_frames = (frames - span).checked_add(stretched_frames);
                let result_sec = result_frames.map_or(f64::INFINITY, |f| f as f64 / rate);
                if result_sec > MAX_OUTPUT_SEC {
                    return Err(format!(
                        "the result would be {result_sec:.0} s long; the longest paulstretch makes is {MAX_OUTPUT_SEC:.0} s. Use a smaller factor or a shorter range"
                    ));
                }

                let stretched = audio_time::paulstretch(
                    &samples[start * stride..end * stride],
                    sample_rate,
                    channels,
                    factor,
                    window_sec,
                    seed,
                )
                .map_err(|e| e.to_string())?;
                let tail = samples.split_off(end * stride);
                samples.truncate(start * stride);
                samples.extend_from_slice(&stretched);
                samples.extend_from_slice(&tail);

                edit_done.set(Done {
                    start_sec: start as f64 / rate,
                    end_sec: end as f64 / rate,
                    window_sec: audio_time::paulstretch::window_len(sample_rate, window_sec) as f64
                        / rate,
                    output_sec: (samples.len() / stride) as f64 / rate,
                });
                Ok((sample_rate, channels))
            },
            move |state, _edited| {
                let d = after_done.get();
                let edit = if range.is_none() {
                    TimeEdit::Scale {
                        factor: factor as f64,
                    }
                } else {
                    // The range lengthens the recording where it ends, which is
                    // what inserting that much time there does: marks, words
                    // and (with sync-lock) every other track follow.
                    TimeEdit::Gap {
                        at: d.end_sec,
                        len: (d.end_sec - d.start_sec) * (factor as f64 - 1.0),
                    }
                };
                apply(state, edit, &scope, &rates);
                let mut out = serde_json::Map::new();
                out.insert("factor".into(), Value::from(factor));
                out.insert("window_sec".into(), Value::from(d.window_sec));
                out.insert("seed".into(), Value::from(seed));
                out.insert("stretched_from_sec".into(), Value::from(d.start_sec));
                out.insert("stretched_to_sec".into(), Value::from(d.end_sec));
                out.insert("output_seconds".into(), Value::from(d.output_sec));
                out
            },
            label,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dispatcher::ToolDispatcher;

    #[test]
    fn schema_requires_a_track_and_a_factor() {
        let s = PaulstretchTool.schema();
        assert_eq!(s["name"], "paulstretch");
        let required: Vec<&str> = s["input_schema"]["required"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(required, ["track", "factor"]);
    }

    #[test]
    fn dispatcher_registers_paulstretch() {
        assert!(ToolDispatcher::default_dispatcher()
            .get("paulstretch")
            .is_some());
    }
}
