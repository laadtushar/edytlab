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
//! ## Where the speech comes from
//!
//! `text` needs the words: finding a phrase means knowing what was said,
//! and only a transcript has that. `speech_passage` needs less. "The
//! last thing said" is a place, not a phrase, so it can come from the
//! transcript or, with `voice_tracks`, from the audio of the tracks that
//! hold the voice, through the detector `duck_under_speech` uses
//! ([`speech_detect`](crate::tool::speech_detect)) and the same rule for
//! joining speech into passages, `join_gap_sec`, read the same way
//! ([`join_gap`]). That matters for a session that has not been transcribed
//! (`transcribe` needs the speech model the user downloads, #384): without
//! it `speech_passage` could only fail there.
//!
//! A track named in `voice_tracks` wins over a transcript, because the
//! transcript is session-level and cannot say which track it belongs to.
//! `voice_tracks` or `join_gap_sec` with `text` or a beat range is
//! refused: neither has passages of speech to find or join, and ignoring
//! the argument would silently answer a different question.
//!
//! ## Off the store lock
//!
//! Reading a voice track decodes every clip on it and scans the whole
//! timeline, which takes as long as the voice is long, and the store,
//! engine and clipboard locks used to be held for all of it (#570). The
//! tool reads the head and appends nothing, so it runs on a staged handle
//! ([`Tool::runs_off_the_lock`]): a user's edit made meanwhile lands at
//! once, and the selection is the one the head the call began on gives, as
//! it was when the edit waited. A run that appends nothing is never run
//! again, so the one selection it reports is the call's own.
//!
//! The audio route tells pitched sound from unpitched sound, not a voice
//! from an instrument, so a voice track that carries music reads as
//! speech throughout; `speech_detect` lists the other limits. Every
//! result says what it keyed on, so the choice can be checked before the
//! next call acts on it.
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
use crate::tool::ducking::{join_gap, join_passages, speech_passages, Passage};
use crate::tool::speech_detect::{speech_in_track, track_list, unique_tracks};
use crate::tool::util::{check_track_index, load_head_state};
use crate::{Tool, ToolContext, ToolResult};

/// What a selector resolved to, and what it was found from.
struct Resolved {
    start_s: f64,
    end_s: f64,
    /// What was matched, in words, for the caller to check.
    matched: String,
    /// `"transcript"`, `"audio"` or `"tempo_map"`.
    keyed_on: &'static str,
    /// The tracks the audio was read from. Empty unless `keyed_on` is
    /// `"audio"`.
    voice_tracks: Vec<usize>,
}

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
    /// Find the passages in these tracks' audio instead of in the
    /// transcript. Only for `speech_passage`.
    #[serde(default)]
    voice_tracks: Option<Vec<usize>>,
    /// Pauses shorter than this are inside one passage. Only for
    /// `speech_passage`. Read as `duck_under_speech` reads its own.
    #[serde(default)]
    join_gap_sec: Option<f32>,
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
             transcript, a voice track's audio, or its tempo map. Give exactly one of: `text` (a \
             phrase to find in the transcript), `speech_passage` (1-based, or negative from the \
             end — -1 is the last thing said), or `from_beat`/`to_beat`. `speech_passage` reads \
             the transcript; with no transcript, pass `voice_tracks` and it finds the speech in \
             those tracks' audio instead, so -1 is the last thing said on them. `text` always \
             needs the transcript, because finding a phrase needs the words. Passages join across \
             pauses shorter than `join_gap_sec`, as in `duck_under_speech`. Returns start_sec \
             and end_sec for any tool that takes a range, and reports what it matched and what \
             it keyed on (transcript, audio or tempo_map), so the choice can be checked before \
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
                    "voice_tracks": {
                        "type": "array",
                        "items": { "type": "integer", "minimum": 0 },
                        "minItems": 1,
                        "description": "Track(s) with the speech, for `speech_passage` when the session has no transcript. Finds the passages where their audio has voiced (pitched) sound instead of in the transcript, so a breath, click or hiss is not a passage; not for `text` or a beat range. The voice alone: a track with music in it reads as speech throughout, and loud low rumble, or a fan that runs for part of the track, reads as speech while it lasts. The detector's thresholds were set on synthetic recordings and not yet checked on real takes, so check the range before a destructive edit.",
                    },
                    "join_gap_sec": {
                        "type": "number",
                        "description": "For `speech_passage`: pauses shorter than this stay inside one passage. Default 1 s, as in `duck_under_speech`; pass the same value to both.",
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

    // Read-only: it resolves a range and reports it. That moves the UI
    // highlight, not the session; showing a region before an edit is its
    // whole purpose.
    fn mutates(&self) -> bool {
        false
    }

    // Reads the head's audio and appends nothing (#570); see the module
    // docs.
    fn runs_off_the_lock(&self) -> bool {
        true
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

        // `voice_tracks` answers where speech is, so it is for the
        // selector that asks for a place. Refused rather than ignored for
        // the others: a phrase needs the words, which audio does not
        // have, and a beat range has no audio in it.
        if args.voice_tracks.is_some() && args.speech_passage.is_none() {
            return Ok(ToolResult::Error(if args.text.is_some() {
                "`voice_tracks` goes with `speech_passage`, not `text`. Finding a phrase needs the \
                 words, which only a transcript has: audio says where speech is, not what was \
                 said. Drop `voice_tracks` and use a transcript, or ask for a `speech_passage`"
                    .to_string()
            } else {
                "`voice_tracks` goes with `speech_passage`, not a beat range: beats come from the \
                 tempo map and there is no speech to look for. Drop `voice_tracks`"
                    .to_string()
            }));
        }
        // The same for `join_gap_sec`, which says how passages of speech
        // are joined: a phrase or a beat range has none.
        if args.join_gap_sec.is_some() && args.speech_passage.is_none() {
            return Ok(ToolResult::Error(
                "`join_gap_sec` goes with `speech_passage`: it says how long a pause splits two \
                 passages of speech, and a phrase or a beat range has no passages to join. Drop \
                 `join_gap_sec`"
                    .to_string(),
            ));
        }

        let state = match load_head_state(ctx) {
            Ok(s) => s,
            Err(msg) => return Ok(ToolResult::Error(msg)),
        };

        let resolved = if let Some(text) = args.text.as_deref() {
            resolve_text(&state, text, args.occurrence.unwrap_or(1))
        } else if let Some(n) = args.speech_passage {
            resolve_passage(&state, n, args.voice_tracks, join_gap(args.join_gap_sec))
        } else {
            resolve_beats(&state, args.from_beat, args.to_beat)
        };
        let Resolved {
            start_s,
            end_s,
            matched,
            keyed_on,
            voice_tracks,
        } = match resolved {
            Ok(r) => r,
            Err(msg) => return Ok(ToolResult::Error(msg)),
        };

        let pad = args.pad_sec.unwrap_or(0.0).max(0.0);
        let start = (start_s - pad).max(0.0);
        let end = end_s + pad;

        // Reported, not applied. The acceptance is that a described
        // region becomes something the user can *check* before a
        // destructive tool is pointed at it.
        crate::progress::report(json!({
            "kind": "selection",
            "start_sec": start,
            "end_sec": end,
            "matched": matched,
        }));

        // Found from sound, not from the words: said where the caller
        // reads it, since the next call is usually a destructive edit.
        let caveat = if keyed_on == "audio" {
            " This came from the audio (voiced sound), not from the words, so check it before a \
             destructive edit."
        } else {
            ""
        };
        let mut out = json!({
            "start_sec": start,
            "end_sec": end,
            "duration_sec": end - start,
            "matched": matched,
            "keyed_on": keyed_on,
            "summary": format!(
                "Selected {start:.2}s–{end:.2}s ({:.2}s): {matched}. Pass these as the range to \
                 any tool that takes one.{caveat}",
                end - start
            ),
        });
        if !voice_tracks.is_empty() {
            out["voice_tracks"] = json!(voice_tracks);
        }
        Ok(ToolResult::Ok(out))
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
) -> Result<Resolved, String> {
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

    // Every match, not just up to the one asked for: what was matched says
    // how many there are, so the caller can tell "the second of two" from
    // "the second of five" (#570).
    let starts: Vec<usize> = (0..hay.len().saturating_sub(needle.len() - 1))
        .filter(|&i| hay[i..i + needle.len()] == needle[..])
        .collect();
    let found = starts.len();
    if let Some(&i) = occurrence.checked_sub(1).and_then(|n| starts.get(n)) {
        let first = &words[i];
        let last = &words[i + needle.len() - 1];
        return Ok(Resolved {
            start_s: first.start_s as f64,
            end_s: last.end_s as f64,
            matched: format!("\"{phrase}\" (occurrence {occurrence} of {found})"),
            keyed_on: "transcript",
            voice_tracks: Vec::new(),
        });
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

/// The `which`-th passage of speech, from the audio of `voice_tracks` when
/// they are given and from the transcript when they are not.
///
/// Both routes join speech across pauses shorter than `join_gap_s` and
/// count the passages the same way, so a passage number means the same
/// from either, and the same as in `duck_under_speech` called with the same
/// `join_gap_sec` (both read it through [`join_gap`]).
fn resolve_passage(
    state: &session::SessionState,
    which: i64,
    voice_tracks: Option<Vec<usize>>,
    join_gap_s: f32,
) -> Result<Resolved, String> {
    let (passages, keyed_on, voice_tracks) = match voice_tracks {
        // A named track wins over a transcript: the transcript is
        // session-level and cannot say which track it belongs to.
        Some(requested) => {
            let tracks = unique_tracks(requested);
            let passages = passages_in_audio(state, &tracks, join_gap_s)?;
            (passages, "audio", tracks)
        }
        None => {
            let Some(transcript) = &state.transcript else {
                return Err(
                    "this session has no transcript; pass `voice_tracks` (the track(s) with the \
                     speech) to find the passage in that audio instead"
                        .to_string(),
                );
            };
            let passages = speech_passages(&transcript.words, join_gap_s);
            if passages.is_empty() {
                return Err("the transcript has no words, so there are no passages".to_string());
            }
            (passages, "transcript", Vec::new())
        }
    };

    let n = passages.len() as i64;
    // Negative counts from the end, so -1 is "the last thing he said"
    // without the caller having to know how many there are.
    let index = if which < 0 { n + which } else { which - 1 };
    if index < 0 || index >= n {
        return Err(format!(
            "there are {n} passage(s) of speech, so passage {which} does not exist (1 is the \
             first and {n} the last; -1 counts back from the end)"
        ));
    }

    let p = passages[index as usize];
    let matched = if voice_tracks.is_empty() {
        format!("passage {} of {n} of speech", index + 1)
    } else {
        format!(
            "passage {} of {n} of speech found in the audio of track(s) {} (voiced sound, so \
             breaths and clicks are ignored)",
            index + 1,
            track_list(&voice_tracks)
        )
    };
    Ok(Resolved {
        start_s: p.start_s as f64,
        end_s: p.end_s as f64,
        matched,
        keyed_on,
        voice_tracks,
    })
}

/// The passages of speech in the audio of `tracks`, in time order.
///
/// Several tracks are one timeline: two voices that overlap are one
/// passage, as they are to `duck_under_speech`.
fn passages_in_audio(
    state: &session::SessionState,
    tracks: &[usize],
    join_gap_s: f32,
) -> Result<Vec<Passage>, String> {
    if tracks.is_empty() {
        return Err("voice_tracks is empty; name the track(s) with the speech".to_string());
    }
    // Every name checked before any audio is read: decoding and analysing
    // a long take is the expensive part, and a typo should not wait on it.
    for &t in tracks {
        check_track_index(&state.tracks, t)?;
    }
    let mut found = Vec::new();
    for &t in tracks {
        found.extend(speech_in_track(&state.tracks[t].clips, t)?);
    }
    let passages = join_passages(found, join_gap_s);
    if passages.is_empty() {
        return Err(format!(
            "found no speech on track(s) {}: nothing voiced stands above that audio's own noise \
             floor, so there is no passage to select",
            track_list(tracks)
        ));
    }
    Ok(passages)
}

fn resolve_beats(
    state: &session::SessionState,
    from: Option<u64>,
    to: Option<u64>,
) -> Result<Resolved, String> {
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

    Ok(Resolved {
        start_s: start,
        end_s: end,
        matched: format!("beats {from}–{to} at {bpm:.1} BPM"),
        keyed_on: "tempo_map",
        voice_tracks: Vec::new(),
    })
}
