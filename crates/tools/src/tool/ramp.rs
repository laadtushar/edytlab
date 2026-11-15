//! Sliding stretch: what `time_stretch` and `pitch_shift` share when they
//! are given an `end_factor` / `end_semitones` or a `range`.
//!
//! The DSP is `audio_time::time_stretch_ramp` / `pitch_shift_ramp`, one
//! vocoder pass whose ratio moves linearly along the input. This module is
//! the session half, modelled on `paulstretch`: it cuts the range out of the
//! track's samples, has the DSP rewrite it, splices the result back, and
//! then moves what the length change moved (marks, words, and with
//! sync-lock the other tracks) in the same node.
//!
//! A stretch ramp changes the span's length, so what lies after it moves by
//! what was gained or lost and a mark inside moves in proportion to where it
//! sat (the integral of the ramp, `audio_time::ramp_output_position`). A pitch
//! ramp changes no length, so nothing moves.

use std::cell::Cell;
use std::rc::Rc;

use serde_json::Value;

use crate::tool::util::{
    cut_timeline, destructive_edit_then, insert_gap_timeline, load_head_state, remap_after_warp,
    seconds_to_frames, sync_other_tracks, track_clip_rates, Moved,
};
use crate::{Range, ToolContext, ToolResult};

/// The longest result of a ramped stretch, in seconds (as `paulstretch`).
const MAX_OUTPUT_SEC: f64 = 1200.0;

/// Which quantity ramps, and between what.
#[derive(Clone, Copy)]
pub(crate) enum Ramp {
    /// Speed factors, as `time_stretch`'s `factor`.
    Stretch { start: f32, end: f32 },
    /// Semitones, as `pitch_shift`'s `semitones`.
    Pitch { start: f32, end: f32 },
}

/// What the edit did, for the hook that moves the records and reports it.
#[derive(Clone, Copy, Default)]
struct Done {
    start_sec: f64,
    end_sec: f64,
    out_span_sec: f64,
    output_sec: f64,
}

/// Run a ramp over `range` (or the whole track) of `track`.
pub(crate) fn run(
    ctx: &mut ToolContext,
    track: usize,
    range: Option<Range>,
    ramp: Ramp,
    preserve_formants: bool,
    label: String,
) -> ToolResult {
    // Sync-lock needs each other track's own rate, asked before anything is
    // edited so a track with no single rate refuses the call.
    let rates = match load_head_state(ctx) {
        Ok(state) if state.sync_lock => match track_clip_rates(&state) {
            Ok(r) => r,
            Err(e) => return ToolResult::Error(e),
        },
        Ok(_) => Vec::new(),
        Err(e) => return ToolResult::Error(e),
    };
    let done = Rc::new(Cell::new(Done::default()));
    let edit_done = Rc::clone(&done);
    let after_done = Rc::clone(&done);

    destructive_edit_then(
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
            if let Ramp::Stretch { start: f0, end: f1 } = ramp {
                let span_f = span as f64;
                let out_span =
                    audio_time::ramp_output_position(span_f, span_f, f0 as f64, f1 as f64).round();
                let result_sec = (frames as f64 - span_f + out_span) / rate;
                if result_sec > MAX_OUTPUT_SEC {
                    return Err(format!(
                        "the result would be {result_sec:.0} s long; the longest a ramped time_stretch makes is {MAX_OUTPUT_SEC:.0} s. Use a shorter range or factors closer to 1"
                    ));
                }
            }
            let piece = &samples[start * stride..end * stride];
            let out = match ramp {
                Ramp::Stretch { start: a, end: b } => audio_time::time_stretch_ramp(
                    piece,
                    sample_rate,
                    channels,
                    a,
                    b,
                    preserve_formants,
                ),
                Ramp::Pitch { start: a, end: b } => audio_time::pitch_shift_ramp(
                    piece,
                    sample_rate,
                    channels,
                    a,
                    b,
                    preserve_formants,
                ),
            }
            .map_err(|e| e.to_string())?;
            let out_frames = out.len() / stride;
            let tail = samples.split_off(end * stride);
            samples.truncate(start * stride);
            samples.extend_from_slice(&out);
            samples.extend_from_slice(&tail);
            edit_done.set(Done {
                start_sec: start as f64 / rate,
                end_sec: end as f64 / rate,
                out_span_sec: out_frames as f64 / rate,
                output_sec: (samples.len() / stride) as f64 / rate,
            });
            Ok((sample_rate, channels))
        },
        move |state, edited| {
            let d = after_done.get();
            let span_sec = d.end_sec - d.start_sec;
            let delta = d.out_span_sec - span_sec;
            if let Ramp::Stretch { start: f0, end: f1 } = ramp {
                let moved = if range.is_some() {
                    Moved::sync_locked(state, edited)
                } else {
                    Moved::track(edited)
                };
                // The ideal integral and the frames actually written differ by
                // under a frame; scale so the span's end lands on the audio's.
                let ideal =
                    audio_time::ramp_output_position(span_sec, span_sec, f0 as f64, f1 as f64);
                let fit = d.out_span_sec / ideal;
                let (s, e) = (d.start_sec, d.end_sec);
                remap_after_warp(state, moved, &|t| {
                    if t <= s {
                        t
                    } else if t < e {
                        s + audio_time::ramp_output_position(t - s, span_sec, f0 as f64, f1 as f64)
                            * fit
                    } else {
                        t + delta
                    }
                });
                if range.is_some() && state.sync_lock {
                    let (at, new_end) = (d.end_sec, d.start_sec + d.out_span_sec);
                    sync_other_tracks(state, edited, &rates, |clips, rate| {
                        if delta >= 0.0 {
                            let len = seconds_to_frames(delta, rate);
                            if len == 0 {
                                return clips.to_vec();
                            }
                            insert_gap_timeline(clips, seconds_to_frames(at, rate), len)
                        } else {
                            let (from, to) = (
                                seconds_to_frames(new_end, rate),
                                seconds_to_frames(at, rate),
                            );
                            if from >= to {
                                return clips.to_vec();
                            }
                            cut_timeline(clips, from, to)
                        }
                    });
                }
            }
            let mut out = serde_json::Map::new();
            let (a, b) = match ramp {
                Ramp::Stretch { start, end } | Ramp::Pitch { start, end } => (start, end),
            };
            out.insert("start_value".into(), Value::from(a));
            out.insert("end_value".into(), Value::from(b));
            out.insert("from_sec".into(), Value::from(d.start_sec));
            out.insert("to_sec".into(), Value::from(d.end_sec));
            out.insert("output_seconds".into(), Value::from(d.output_sec));
            out
        },
        label,
    )
}
