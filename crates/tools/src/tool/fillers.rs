//! `remove_fillers` — "um", "uh", "you know" (#165).
//!
//! One of the most-performed edits in podcast production and one of the
//! most tedious by hand. It is also the clearest demonstration that an
//! editor understands *speech* rather than *waveforms*: it needs to
//! know where the words are, which since #157 it does.
//!
//! The code is a filter over `TranscriptWord` plus the cut that
//! `cut_words` already performs. The judgement is the interesting part,
//! and there are four decisions worth stating out loud.
//!
//! ## 1. It reports before it acts
//!
//! The default is a dry run. This is a destructive edit across a whole
//! track, and "undo is available" is not the same as the user having
//! agreed — so the tool says *"found 47 fillers, removing them saves 31
//! seconds"* and waits to be asked again with `apply: true`.
//!
//! ## 2. It does not remove every one
//!
//! Speech with all hesitation stripped sounds unnatural and rushed.
//! Hesitations proper — "um", "uh", "er" — are almost always removable.
//! Discourse markers — "like", "you know", "I mean" — are only removed
//! when they stand alone between pauses, because mid-sentence they are
//! carrying rhythm rather than filling a gap.
//!
//! ## 3. It leaves a gap where the breath was
//!
//! Cutting a filler dead against the next word makes speech sound
//! spliced. A short pause is retained out of the filler's own time, so
//! the rhythm survives the edit.
//!
//! ## 4. One node
//!
//! Forty-seven fillers is one edit, not forty-seven. Undo puts the
//! whole take back.

use serde::Deserialize;
use serde_json::{json, Value};

use crate::schema::anthropic_tool;
use crate::tool::util::{
    append_state, check_track_index, cut_annotations, cut_timeline, load_head_state,
};
use crate::{Tool, ToolContext, ToolResult};

/// Hesitations. These carry no meaning and are removable wherever they
/// appear.
const HESITATIONS: &[&str] = &["um", "uh", "erm", "er", "ah", "eh", "hmm", "mm", "mmm"];

/// Discourse markers. Removable only when they stand alone — between
/// pauses — because in the middle of a sentence they are doing work.
const MARKERS: &[&str] = &["like", "basically", "actually", "literally", "right"];

/// A pause long enough to say a word is standing alone rather than
/// running into its neighbours.
const STANDALONE_PAUSE_S: f32 = 0.20;

/// How much of a filler's time to leave behind as a pause, so the
/// result does not sound spliced.
const DEFAULT_KEEP_GAP_MS: f32 = 80.0;

#[derive(Debug, Deserialize)]
struct Args {
    /// Actually remove them. Absent or false only reports.
    #[serde(default)]
    apply: bool,
    /// Replaces the built-in list entirely. Fillers are language- and
    /// speaker-specific, and one person's filler is another's emphasis.
    #[serde(default)]
    words: Option<Vec<String>>,
    /// Milliseconds of pause to leave where each filler was.
    #[serde(default)]
    keep_gap_ms: Option<f32>,
    #[serde(default)]
    track: Option<usize>,
}

pub struct RemoveFillersTool;

impl Tool for RemoveFillersTool {
    fn name(&self) -> &'static str {
        "remove_fillers"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "remove_fillers",
            "Find filler words in the session's transcript and, when asked, remove them and their \
             audio in one undoable edit. Reports by default without changing anything: this is a \
             destructive edit across a whole track, so it says what it found and waits. Removes \
             hesitations (um, uh, er) wherever they appear, but discourse markers (like, \
             actually) only where they stand alone between pauses — speech with every hesitation \
             stripped sounds rushed. Leaves a short pause where each filler was so the result \
             does not sound spliced.",
            json!({
                "type": "object",
                "properties": {
                    "apply": {
                        "type": "boolean",
                        "description": "Remove them. Omit to report what would be removed without touching the session.",
                    },
                    "words": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Replaces the built-in list. Fillers are language- and speaker-specific.",
                    },
                    "keep_gap_ms": {
                        "type": "number",
                        "description": "Pause to leave where each filler was, in milliseconds. Default 80.",
                    },
                    "track": { "type": "integer", "description": "Track the transcript describes; defaults to 0" },
                },
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
        let keep_gap_s = args.keep_gap_ms.unwrap_or(DEFAULT_KEEP_GAP_MS).max(0.0) / 1000.0;

        let mut state = match load_head_state(ctx) {
            Ok(s) => s,
            Err(msg) => return Ok(ToolResult::Error(msg)),
        };
        if let Err(msg) = check_track_index(&state.tracks, track_index) {
            return Ok(ToolResult::Error(msg));
        }
        let Some(transcript) = state.transcript.clone() else {
            return Ok(ToolResult::Error(
                "this session has no transcript; run `transcribe` first so words have timings"
                    .to_string(),
            ));
        };

        let custom: Option<Vec<String>> = args
            .words
            .map(|w| w.iter().map(|s| normalise(s)).collect::<Vec<_>>());
        let found = find_fillers(&transcript.words, custom.as_deref());

        let removed_sec: f32 = found
            .iter()
            .map(|f| (f.end_s - f.start_s - keep_gap_s).max(0.0))
            .sum();

        if found.is_empty() {
            return Ok(ToolResult::Ok(json!({
                "found": 0,
                "applied": false,
                "summary": "No filler words found in this transcript.",
            })));
        }

        // Report first. A destructive edit across a whole track is not
        // something to do because it was mentioned.
        if !args.apply {
            return Ok(ToolResult::Ok(json!({
                "found": found.len(),
                "applied": false,
                "would_save_sec": removed_sec,
                "words": found.iter().map(|f| json!({
                    "index": f.index,
                    "text": f.text,
                    "start_sec": f.start_s,
                    "end_sec": f.end_s,
                })).collect::<Vec<_>>(),
                "summary": format!(
                    "Found {} filler{} ({}), removing them saves {:.1}s. Nothing was changed — \
                     call again with apply: true to remove them.",
                    found.len(),
                    if found.len() == 1 { "" } else { "s" },
                    summarise(&found),
                    removed_sec,
                ),
            })));
        }

        // Apply every cut in one pass, back to front. Working backwards
        // means earlier spans keep their original times: a forward pass
        // would need every remaining span adjusted by what came before,
        // which is the same arithmetic done more often and wrong more
        // easily.
        let sr = state.sample_rate.max(1) as f64;
        let mut words = transcript.words.clone();
        let mut total_removed_s = 0.0f32;
        let mut dropped_labels = 0usize;

        for filler in found.iter().rev() {
            let cut_start = filler.start_s;
            let cut_end = (filler.end_s - keep_gap_s).max(cut_start);
            if cut_end <= cut_start {
                // The filler is shorter than the pause we would leave;
                // removing it would gain nothing audible.
                continue;
            }

            let start_frame = (cut_start as f64 * sr).round() as u64;
            let end_frame = (cut_end as f64 * sr).round() as u64;
            let track = &mut state.tracks[track_index];
            track.clips = cut_timeline(&track.clips, start_frame, end_frame);

            let span = cut_end - cut_start;
            total_removed_s += span;

            // The labels move with the audio (#231). `words` below is
            // shifted per iteration, so these coordinates are the
            // *current* timeline's — and the annotations have to be cut
            // in the same evolving space, one span at a time.
            let (kept, dropped) =
                cut_annotations(&state.annotations, cut_start as f64, cut_end as f64);
            state.annotations = kept;
            dropped_labels += dropped;

            words.remove(filler.index);
            for w in words.iter_mut().skip(filler.index) {
                w.start_s -= span;
                w.end_s -= span;
            }
        }

        state.transcript = Some(session::Transcript { words });
        state.length_samples = state
            .length_samples
            .saturating_sub((total_removed_s as f64 * sr).round() as u64);

        let new_id = match append_state(ctx, state, format!("remove {} filler words", found.len()))
        {
            Ok(id) => id,
            Err(msg) => return Ok(ToolResult::Error(msg)),
        };

        Ok(ToolResult::Ok(json!({
            "node_id": new_id.to_hex(),
            "found": found.len(),
            "applied": true,
            "removed_sec": total_removed_s,
            // Silently discarding a user's chapter mark is the one
            // outcome worth naming; `cut_range` has always reported it.
            "dropped_labels": dropped_labels,
            "summary": format!(
                "Removed {} filler{} ({}), {:.1}s shorter. One undoable edit, with a {:.0}ms \
                 pause left where each one was.",
                found.len(),
                if found.len() == 1 { "" } else { "s" },
                summarise(&found),
                total_removed_s,
                keep_gap_s * 1000.0,
            ),
        })))
    }
}

/// One filler, and where it is.
#[derive(Debug, Clone, PartialEq)]
pub struct Filler {
    pub index: usize,
    pub text: String,
    pub start_s: f32,
    pub end_s: f32,
}

/// Strip punctuation and case so "Um," matches "um".
fn normalise(word: &str) -> String {
    word.chars()
        .filter(|c| c.is_alphanumeric() || *c == '\'')
        .collect::<String>()
        .to_lowercase()
}

/// Which words to remove.
///
/// Hesitations go wherever they are. Discourse markers go only where
/// they stand alone — a pause on both sides — because mid-sentence they
/// are carrying rhythm rather than filling a gap, and stripping those
/// is what makes edited speech sound rushed.
pub fn find_fillers(words: &[session::TranscriptWord], custom: Option<&[String]>) -> Vec<Filler> {
    let mut out = Vec::new();

    for (i, word) in words.iter().enumerate() {
        let text = normalise(&word.text);
        if text.is_empty() {
            continue;
        }

        let removable = match custom {
            // A caller-supplied list is taken at its word: they know
            // their speaker better than this heuristic does.
            Some(list) => list.contains(&text),
            None => {
                if HESITATIONS.contains(&text.as_str()) {
                    true
                } else if MARKERS.contains(&text.as_str()) {
                    stands_alone(words, i)
                } else {
                    false
                }
            }
        };

        if removable {
            out.push(Filler {
                index: i,
                text: word.text.clone(),
                start_s: word.start_s,
                end_s: word.end_s,
            });
        }
    }

    out
}

/// A word stands alone when there is a pause on both sides of it. At
/// the very start or end of a take, the missing side counts as a pause.
fn stands_alone(words: &[session::TranscriptWord], i: usize) -> bool {
    let before = match i.checked_sub(1).and_then(|p| words.get(p)) {
        Some(prev) => words[i].start_s - prev.end_s >= STANDALONE_PAUSE_S,
        None => true,
    };
    let after = match words.get(i + 1) {
        Some(next) => next.start_s - words[i].end_s >= STANDALONE_PAUSE_S,
        None => true,
    };
    before && after
}

/// "um ×3, like ×1" — what was found, at a glance.
fn summarise(found: &[Filler]) -> String {
    let mut counts: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    for f in found {
        *counts.entry(normalise(&f.text)).or_default() += 1;
    }
    counts
        .into_iter()
        .map(|(word, n)| {
            if n == 1 {
                word
            } else {
                format!("{word} ×{n}")
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}
