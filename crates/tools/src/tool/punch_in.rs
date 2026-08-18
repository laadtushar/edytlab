//! `punch_in` — drop a retake into a region without disturbing the rest
//! (#203 §2).
//!
//! The thing this exists for: one line was misread. Everything else in
//! the take is fine. Re-recording the whole pass to fix eight seconds is
//! the cost punch-in removes.
//!
//! ## Replace, not insert
//!
//! The region's length does not change. That is what makes it *punch-in*
//! rather than an edit: everything after the punch stays exactly where it
//! was, so the music underneath still lines up, the labels still point at
//! the right words, and a multitrack session does not desynchronise.
//!
//! A take that does not match the region is fitted rather than allowed to
//! ripple — trimmed if long, padded with silence if short — and the tool
//! says by how much. Silently stretching it would change the performance;
//! silently rippling would move everything downstream, which is precisely
//! what the user asked not to happen by punching in.

use serde::Deserialize;
use serde_json::{json, Value};

use crate::schema::anthropic_tool;
use crate::tool::util::{
    check_seconds_order, check_track_index, destructive_edit, load_head_state, track_channels,
};
use crate::{Tool, ToolContext, ToolResult};

/// Splice `take` into `samples` over `[start_sec, end_sec)`.
///
/// Both buffers are interleaved at `channels`. Returns how many frames
/// of the take were used and how many frames of the region the take
/// could not fill, so the caller can report a trim or a pad rather than
/// leaving the user to notice.
pub(crate) fn apply_punch(
    samples: &mut [f32],
    take: &[f32],
    sr: u32,
    channels: usize,
    start_sec: f64,
    end_sec: f64,
) -> (usize, usize) {
    let stride = channels.max(1);
    let start = ((start_sec * sr as f64).max(0.0) as usize * stride).min(samples.len());
    let end = ((end_sec * sr as f64).max(0.0) as usize * stride).min(samples.len());
    if end <= start {
        return (0, 0);
    }

    let region = end - start;
    let used = take.len().min(region);
    samples[start..start + used].copy_from_slice(&take[..used]);
    // A short take leaves the tail of the region silent rather than
    // leaving the old performance audible behind it. Half a retake over
    // half the original is the one outcome nobody wants.
    for s in &mut samples[start + used..end] {
        *s = 0.0;
    }

    (used / stride, (region - used) / stride)
}

#[derive(Debug, Deserialize)]
struct Args {
    track: usize,
    start_sec: f64,
    end_sec: f64,
    /// The retake — usually what `stop_recording` just wrote.
    take_path: String,
}

pub struct PunchInTool;

impl Tool for PunchInTool {
    fn name(&self) -> &'static str {
        "punch_in"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "punch_in",
            "Replace a region of a track with audio from a file, in place. The region's length \
             is unchanged, so everything after the punch stays where it was — this is how a \
             misread line gets fixed without re-recording the take or shifting the rest of the \
             session. A take longer than the region is trimmed; a shorter one is padded with \
             silence, and the tool reports which happened. Appends one undoable node.",
            json!({
                "type": "object",
                "properties": {
                    "track": { "type": "integer", "description": "Zero-based track index" },
                    "start_sec": { "type": "number", "description": "Start of the punch region" },
                    "end_sec": { "type": "number", "description": "End of the punch region" },
                    "take_path": {
                        "type": "string",
                        "description": "Audio file holding the retake, e.g. what stop_recording wrote.",
                    },
                },
                "required": ["track", "start_sec", "end_sec", "take_path"],
                "additionalProperties": false,
            }),
        )
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };

        if let Err(msg) = check_seconds_order(args.start_sec, args.end_sec) {
            return Ok(ToolResult::Error(msg));
        }
        let (start_sec, end_sec) = (args.start_sec, args.end_sec);

        let state = match load_head_state(ctx) {
            Ok(s) => s,
            Err(msg) => return Ok(ToolResult::Error(msg)),
        };
        if let Err(msg) = check_track_index(&state.tracks, args.track) {
            return Ok(ToolResult::Error(msg));
        }

        let take = match audio_decoder::decode_file(std::path::Path::new(&args.take_path)) {
            Ok(t) => t,
            Err(e) => {
                return Ok(ToolResult::Error(format!(
                    "could not read the take at {}: {e}",
                    args.take_path
                )))
            }
        };

        // A take at a different rate would play at the wrong speed and
        // pitch once spliced in, which is a worse outcome than a refusal
        // the user can act on.
        if take.sample_rate != state.sample_rate {
            return Ok(ToolResult::Error(format!(
                "the take is {} Hz but the session is {} Hz; resample it first — splicing it in \
                 as-is would play it at the wrong speed and pitch",
                take.sample_rate, state.sample_rate
            )));
        }

        // The stride belongs to the buffer being written into, not to
        // the take. Getting this from the take is how a mono retake
        // spliced into a stereo track lands at half the intended
        // position and swaps the channels for the rest of the region.
        let track_ch = match track_channels(ctx, args.track) {
            Ok(c) => c,
            Err(msg) => return Ok(ToolResult::Error(msg)),
        };

        // And a take whose interleaving differs from the track's cannot
        // be spliced in at any stride — the samples themselves are laid
        // out differently. Refusing names something the user can fix;
        // guessing produces audio they have to catch by ear.
        if take.channels.max(1) as usize != track_ch {
            return Ok(ToolResult::Error(format!(
                "the take is {}-channel but track {} is {}-channel; convert it first — the two \
                 interleave differently and splicing one into the other would scramble it",
                take.channels.max(1),
                args.track,
                track_ch,
            )));
        }

        let take_sec = take.samples.len() as f64
            / (take.sample_rate.max(1) as f64 * take.channels.max(1) as f64);
        let region_sec = end_sec - start_sec;
        let take_samples = take.samples.clone();

        let mut padded = 0usize;
        let result = destructive_edit(
            ctx,
            args.track,
            |samples, sr| {
                let (_used, p) =
                    apply_punch(samples, &take_samples, sr, track_ch, start_sec, end_sec);
                padded = p;
            },
            format!(
                "punch in {:.2}s–{:.2}s on track {}",
                start_sec, end_sec, args.track
            ),
        );

        let ToolResult::Ok(mut v) = result else {
            return Ok(result);
        };

        let trimmed = (take_sec - region_sec).max(0.0);
        if let Some(obj) = v.as_object_mut() {
            obj.insert("region_sec".into(), json!(region_sec));
            obj.insert("take_sec".into(), json!(take_sec));
            obj.insert("trimmed_sec".into(), json!(trimmed));
            obj.insert(
                "padded_sec".into(),
                json!(padded as f64 / state.sample_rate.max(1) as f64),
            );
            obj.insert(
                "summary".into(),
                json!(format!(
                    "Punched {:.2}s–{:.2}s on track {}.{}",
                    start_sec,
                    end_sec,
                    args.track,
                    if trimmed > 0.01 {
                        format!(
                            " The take was {trimmed:.2}s longer than the region and was trimmed."
                        )
                    } else if padded > 0 {
                        format!(
                            " The take was {:.2}s shorter than the region; the rest is silent.",
                            padded as f64 / state.sample_rate.max(1) as f64
                        )
                    } else {
                        String::new()
                    }
                )),
            );
        }
        Ok(ToolResult::Ok(v))
    }
}
