//! `select_region` — turn a description into a time range (#168 §3).
//!
//! Selection is mouse-only today, which quietly caps what the agent can
//! do: **every range-taking tool already exists**, and none of them can
//! be reached by describing where to apply it. Making selection
//! resolvable makes the whole toolbox describable in the same breath,
//! without touching a single one of them.
//!
//! ## Why this takes a query rather than a sentence
//!
//! The model is already good at understanding "the bit where he talks
//! about latency". What it cannot do is *look it up* — it has no access
//! to the transcript's word timings, the tempo map, or where the speech
//! actually stops. So the division is: the model turns language into a
//! query, and this turns the query into a range it could not have
//! guessed.
//!
//! Putting the language understanding in here instead would mean
//! reimplementing, badly, the thing calling it.
//!
//! ## Why it refuses rather than approximates
//!
//! A selection that is nearly right is worse than none: the next tool
//! call applies a destructive edit to it. So a phrase that is not in
//! the transcript, or a beat past the end of the tempo map, is an error
//! naming the problem — never the closest thing found.

use serde::Deserialize;
use serde_json::{json, Value};

use crate::schema::anthropic_tool;
use crate::tool::ducking::speech_passages;
use crate::tool::util::load_head_state;
use crate::{Tool, ToolContext, ToolResult};

/// Gaps shorter than this are inside one passage of speech rather than
/// between two — the same threshold `duck_under_speech` uses, so "the
/// third thing he said" means the same in both.
const JOIN_GAP_S: f32 = 1.0;

#[derive(Debug, Deserialize)]
struct Args {
    /// A phrase to find in the transcript.
    #[serde(default)]
    text: Option<String>,
    /// Which occurrence of `text`, 1-based. Default 1.
    #[serde(default)]
    occurrence: Option<usize>,
    /// A passage of speech: 1-based, or negative from the end
    /// (-1 is the last thing said).
    #[serde(default)]
    speech_passage: Option<i64>,
    /// A beat range from the session's tempo map, 1-based.
    #[serde(default)]
    from_beat: Option<u64>,
    #[serde(default)]
    to_beat: Option<u64>,
    /// Seconds of padding either side, for a selection that needs to
    /// breathe. Default 0.
    #[serde(default)]
    pad_sec: Option<f64>,
}

pub struct SelectRegionTool;

impl Tool for SelectRegionTool {
    fn name(&self) -> &'static str {
        "select_region"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "select_region",
            "Resolve a description of a region into a concrete time range, using the session's \
             transcript and tempo map. Give exactly one of: `text` (a phrase to find in the \
             transcript), `speech_passage` (1-based, or negative from the end — -1 is the last \
             thing said), or `from_beat`/`to_beat`. Returns start_sec and end_sec for any tool \
             that takes a range, and reports what it matched so the choice can be checked before \
             acting on it. Refuses rather than guessing when the description does not resolve.",
            json!({
                "type": "object",
                "properties": {
                    "text": {
                        "type": "string",
                        "description": "Phrase to find in the transcript, case-insensitive.",
                    },
                    "occurrence": {
                        "type": "integer",
                        "minimum": 1,
                        "description": "Which occurrence of `text`. Default 1.",
                    },
                    "speech_passage": {
                        "type": "integer",
                        "description":
                            "A stretch of continuous speech. 1 is the first, -1 the last.",
                    },
                    "from_beat": { "type": "integer", "minimum": 1 },
                    "to_beat": { "type": "integer", "minimum": 1 },
                    "pad_sec": {
                        "type": "number",
                        "description": "Seconds of headroom either side. Default 0.",
                    },
                },
                "required": [],
                "additionalProperties": false,
            }),
        )
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };

        // Exactly one selector. Accepting several and picking one would
        // silently ignore part of what was asked for.
        let selectors = [
            args.text.is_some(),
            args.speech_passage.is_some(),
            args.from_beat.is_some() || args.to_beat.is_some(),
        ]
        .iter()
        .filter(|b| **b)
        .count();
        if selectors == 0 {
            return Ok(ToolResult::Error(
                "say what to select: `text`, `speech_passage`, or `from_beat`/`to_beat`"
                    .to_string(),
            ));
        }
        if selectors > 1 {
            return Ok(ToolResult::Error(
                "give one selector, not several — `text`, `speech_passage` and `from_beat` mean \
                 different regions and there is no sensible way to combine them"
                    .to_string(),
            ));
        }

        let state = match load_head_state(ctx) {
            Ok(s) => s,
            Err(msg) => return Ok(ToolResult::Error(msg)),
        };

        let (start, end, matched) = if let Some(text) = args.text.as_deref() {
            match resolve_text(&state, text, args.occurrence.unwrap_or(1)) {
                Ok(v) => v,
                Err(msg) => return Ok(ToolResult::Error(msg)),
            }
        } else if let Some(n) = args.speech_passage {
            match resolve_passage(&state, n) {
                Ok(v) => v,
                Err(msg) => return Ok(ToolResult::Error(msg)),
            }
        } else {
            match resolve_beats(&state, args.from_beat, args.to_beat) {
                Ok(v) => v,
                Err(msg) => return Ok(ToolResult::Error(msg)),
            }
        };

        let pad = args.pad_sec.unwrap_or(0.0).max(0.0);
        let start = (start - pad).max(0.0);
        let end = end + pad;

        // Reported, not applied. The acceptance is that a described
        // region becomes something the user can *check* before a
        // destructive tool is pointed at it.
        crate::progress::report(json!({
            "kind": "selection",
            "start_sec": start,
            "end_sec": end,
            "matched": matched,
        }));

        Ok(ToolResult::Ok(json!({
            "start_sec": start,
            "end_sec": end,
            "duration_sec": end - start,
            "matched": matched,
            "summary": format!(
                "Selected {start:.2}s–{end:.2}s ({:.2}s): {matched}. Pass these as the range to \
                 any tool that takes one.",
                end - start
            ),
        })))
    }
}

/// The span of the words that match `phrase`.
///
/// Matched over the word sequence rather than over a joined string, so
/// the result is a range of *words* with real timings — a substring
/// match into concatenated text would give a character offset nothing
/// can use.
fn resolve_text(
    state: &session::SessionState,
    phrase: &str,
    occurrence: usize,
) -> Result<(f64, f64, String), String> {
    let Some(transcript) = &state.transcript else {
        return Err(
            "this session has no transcript; run `transcribe` first — selecting by what was said \
             needs to know what was said"
                .to_string(),
        );
    };
    let needle: Vec<String> = phrase
        .split_whitespace()
        .map(normalise)
        .filter(|w| !w.is_empty())
        .collect();
    if needle.is_empty() {
        return Err("`text` is empty".to_string());
    }

    let words = &transcript.words;
    let hay: Vec<String> = words.iter().map(|w| normalise(&w.text)).collect();

    let mut found = 0usize;
    for i in 0..hay.len().saturating_sub(needle.len() - 1) {
        if hay[i..i + needle.len()] == needle[..] {
            found += 1;
            if found == occurrence {
                let first = &words[i];
                let last = &words[i + needle.len() - 1];
                return Ok((
                    first.start_s as f64,
                    last.end_s as f64,
                    format!("\"{phrase}\" (occurrence {occurrence} of ...)"),
                ));
            }
        }
    }

    if found == 0 {
        Err(format!(
            "\"{phrase}\" is not in the transcript. Nothing was selected rather than the nearest \
             thing found — a selection that is nearly right is worse than none, because the next \
             edit lands on it."
        ))
    } else {
        Err(format!(
            "\"{phrase}\" appears {found} time(s), so occurrence {occurrence} does not exist"
        ))
    }
}

/// Lowercased and stripped of the punctuation transcribers add, so
/// "latency," matches "latency".
fn normalise(w: &str) -> String {
    w.trim_matches(|c: char| !c.is_alphanumeric())
        .to_lowercase()
}

fn resolve_passage(
    state: &session::SessionState,
    which: i64,
) -> Result<(f64, f64, String), String> {
    let Some(transcript) = &state.transcript else {
        return Err(
            "this session has no transcript; run `transcribe` first — selecting a passage of \
             speech needs to know where the speech is"
                .to_string(),
        );
    };
    let passages = speech_passages(&transcript.words, JOIN_GAP_S);
    if passages.is_empty() {
        return Err("the transcript has no words, so there are no passages".to_string());
    }

    let n = passages.len() as i64;
    // Negative counts from the end, so -1 is "the last thing he said"
    // without the caller having to know how many there are.
    let index = if which < 0 { n + which } else { which - 1 };
    if index < 0 || index >= n {
        return Err(format!(
            "there are {n} passage(s) of speech, so {which} does not exist"
        ));
    }

    let p = passages[index as usize];
    Ok((
        p.start_s as f64,
        p.end_s as f64,
        format!("passage {} of {n} of speech", index + 1),
    ))
}

fn resolve_beats(
    state: &session::SessionState,
    from: Option<u64>,
    to: Option<u64>,
) -> Result<(f64, f64, String), String> {
    let (Some(from), Some(to)) = (from, to) else {
        return Err("a beat range needs both `from_beat` and `to_beat`".to_string());
    };
    if to <= from {
        return Err(format!(
            "`to_beat` must be after `from_beat`; got {from} to {to}"
        ));
    }

    let bpm = state.tempo_map.default_bpm;
    if !bpm.is_finite() || bpm <= 0.0 {
        return Err(format!(
            "the session's tempo is {bpm}, which is not a tempo — run `analyze_track` to detect it"
        ));
    }
    let per_beat = 60.0 / bpm;
    // Beats are 1-based for the caller: "bar 1 beat 1" is the start.
    let start = (from - 1) as f64 * per_beat;
    let end = (to - 1) as f64 * per_beat;

    Ok((start, end, format!("beats {from}–{to} at {bpm:.1} BPM")))
}
