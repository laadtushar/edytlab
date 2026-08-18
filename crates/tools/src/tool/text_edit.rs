//! `cut_words` — edit the audio by editing the transcript (#157).
//!
//! Show the transcript, delete a sentence in the text, and the audio is
//! cut to match. That is the whole reason people buy Descript, and the
//! substrate for it was already here: `transcribe` runs Whisper
//! on-device and `SessionState::transcript` carries word-level timings
//! that survive save and load.
//!
//! What was missing is the mapping — a span of words to a span of
//! samples — and what happens to the words that remain.
//!
//! ## One node, not two
//!
//! The cut and the transcript update are the same edit, so they are one
//! append. Doing the cut with `cut_range` and then repairing the
//! transcript separately would leave a node in the middle whose audio
//! and transcript disagree, and an undo that lands on it.
//!
//! ## Re-syncing the words
//!
//! After a cut, every word after it is early by exactly the removed
//! duration. Shifting the stored timings is cheap and exact — the cut
//! length is known — where re-running Whisper would be neither. Words
//! inside the removed span go with it.
//!
//! The alternative, re-transcribing, would also be *wrong* in a subtle
//! way: it would produce new timings for material that has not changed,
//! and a second transcription of the same audio does not agree with the
//! first to the sample.

use serde::Deserialize;
use serde_json::{json, Value};

use crate::schema::anthropic_tool;
use crate::tool::util::{
    append_state, check_track_index, cut_annotations, cut_timeline, load_head_state,
};
use crate::{Tool, ToolContext, ToolResult};

#[derive(Debug, Deserialize)]
struct Args {
    /// First word to remove, zero-based.
    from_word: usize,
    /// One past the last word to remove.
    to_word: usize,
    /// Which track the transcript describes. Defaults to 0, which is
    /// the single-track case this exists for.
    #[serde(default)]
    track: Option<usize>,
}

pub struct CutWordsTool;

impl Tool for CutWordsTool {
    fn name(&self) -> &'static str {
        "cut_words"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "cut_words",
            "Delete a span of transcribed words and the audio underneath it, closing the gap. \
             Indices are into the session's transcript, as returned by `transcribe`. The \
             remaining word timings are shifted so they still line up with the audio, and the \
             whole thing is one undoable node. Requires a transcript.",
            json!({
                "type": "object",
                "properties": {
                    "from_word": { "type": "integer", "minimum": 0, "description": "First word to remove" },
                    "to_word": { "type": "integer", "minimum": 0, "description": "One past the last word to remove" },
                    "track": { "type": "integer", "description": "Track the transcript describes; defaults to 0" },
                },
                "required": ["from_word", "to_word"],
                "additionalProperties": false,
            }),
        )
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };
        let track_index = args.track.unwrap_or(0);

        let mut state = match load_head_state(ctx) {
            Ok(s) => s,
            Err(msg) => return Ok(ToolResult::Error(msg)),
        };
        if let Err(msg) = check_track_index(&state.tracks, track_index) {
            return Ok(ToolResult::Error(msg));
        }

        // A track with no transcript says what to do rather than
        // failing blank: the mapping this tool needs simply is not
        // there yet.
        let Some(transcript) = state.transcript.clone() else {
            return Ok(ToolResult::Error(
                "this session has no transcript; run `transcribe` first so words have timings"
                    .to_string(),
            ));
        };
        let words = &transcript.words;
        if words.is_empty() {
            return Ok(ToolResult::Error(
                "the transcript is empty; there are no words to cut".to_string(),
            ));
        }
        if args.to_word <= args.from_word {
            return Ok(ToolResult::Error(format!(
                "to_word ({}) must be greater than from_word ({})",
                args.to_word, args.from_word
            )));
        }
        if args.to_word > words.len() {
            return Ok(ToolResult::Error(format!(
                "the transcript has {} words; asked to cut up to {}",
                words.len(),
                args.to_word
            )));
        }

        // The span the words cover: from the first word's start to the
        // last one's end. Cutting from the first word's *end* would
        // leave its tail behind, which is audible as a clipped syllable.
        let start_s = words[args.from_word].start_s as f64;
        let end_s = words[args.to_word - 1].end_s as f64;
        if end_s <= start_s {
            return Ok(ToolResult::Error(format!(
                "those words cover no time ({start_s:.3}s to {end_s:.3}s); the transcript may be \
                 damaged"
            )));
        }

        let sr = state.sample_rate.max(1) as f64;
        let start_frame = (start_s * sr).round() as u64;
        let end_frame = (end_s * sr).round() as u64;

        // The audio. `cut_timeline` closes the gap by sliding
        // everything after the cut leftwards, which is the same
        // operation `cut_range` performs.
        let track = &mut state.tracks[track_index];
        track.clips = cut_timeline(&track.clips, start_frame, end_frame);

        let removed_s = end_s - start_s;
        let removed_frames = end_frame.saturating_sub(start_frame);

        // The labels. Cutting words is cutting audio, so a chapter mark
        // after the removed span is now earlier by the same amount
        // (#203) — the transcript below already gets this treatment and
        // the annotations deserve the same.
        let (kept_labels, dropped_labels) = cut_annotations(&state.annotations, start_s, end_s);
        state.annotations = kept_labels;

        // The words. Everything after the cut is now earlier by exactly
        // the removed duration; everything inside it is gone.
        let removed_text: Vec<String> = words[args.from_word..args.to_word]
            .iter()
            .map(|w| w.text.clone())
            .collect();
        let kept: Vec<session::TranscriptWord> = words
            .iter()
            .enumerate()
            .filter(|(i, _)| *i < args.from_word || *i >= args.to_word)
            .map(|(i, w)| {
                if i < args.from_word {
                    w.clone()
                } else {
                    session::TranscriptWord {
                        text: w.text.clone(),
                        start_s: w.start_s - removed_s as f32,
                        end_s: w.end_s - removed_s as f32,
                        confidence: w.confidence,
                    }
                }
            })
            .collect();
        state.transcript = Some(session::Transcript { words: kept });

        state.length_samples = state.length_samples.saturating_sub(removed_frames);

        let new_id = match append_state(
            ctx,
            state,
            format!(
                "cut words {}–{} ({:.2}s)",
                args.from_word,
                args.to_word - 1,
                removed_s
            ),
        ) {
            Ok(id) => id,
            Err(msg) => return Ok(ToolResult::Error(msg)),
        };

        Ok(ToolResult::Ok(json!({
            "node_id": new_id.to_hex(),
            "removed_words": removed_text.len(),
            "removed_text": removed_text.join(" "),
            "removed_sec": removed_s,
            "dropped_labels": dropped_labels,
            "start_sec": start_s,
            "end_sec": end_s,
            "summary": format!(
                "Cut {} word{} (\"{}\") from {:.2}s to {:.2}s, {:.2}s of audio. The remaining \
                 word timings were shifted to match.",
                removed_text.len(),
                if removed_text.len() == 1 { "" } else { "s" },
                truncate(&removed_text.join(" "), 60),
                start_s,
                end_s,
                removed_s,
            ),
        })))
    }
}

/// Keep a summary readable when a whole paragraph was cut.
fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let head: String = s.chars().take(max.saturating_sub(1)).collect();
    format!("{head}…")
}
