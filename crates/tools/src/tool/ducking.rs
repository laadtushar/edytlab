//! `duck_under_speech` — music under voice, keyed on where the speech is
//! (#168).
//!
//! The classic way to do this is a sidechain compressor triggered by
//! the voice track's *level*, which mistakes a breath for speech and
//! misses a quiet line entirely.
//!
//! edytlab keys on where the speech *is*, by one of two routes:
//!
//! * **The transcript**, when the session has one: the word spans are
//!   the speech boundaries, known rather than inferred. This is the
//!   most accurate key there is.
//! * **A voice track's audio**, when the caller names `voice_tracks`:
//!   the passages are where that audio has voiced (pitched) sound, from
//!   [`speech_detect`](crate::tool::speech_detect). It needs no
//!   transcript, which matters while `transcribe` is a stub (#384), and
//!   a breath, click or hiss is not pitched, so none of them triggers a
//!   duck. Naming the track wins over a transcript: the transcript is
//!   session-level and cannot say which track it belongs to.
//!
//! Either way it can duck slightly *before* a line starts, which is
//! what a human engineer does and a sidechain cannot: a level trigger
//! only knows the line began after it has.
//!
//! ## The output is an automation curve, not an effect
//!
//! It writes the same per-clip volume envelope the automation lane
//! already draws and the user can already drag. So the result is
//! visible and editable rather than a black box, and the render path
//! needed no changes at all.

use std::ops::Range;

use serde::Deserialize;
use serde_json::{json, Value};

use crate::schema::anthropic_tool;
use crate::tool::speech_detect::detect_speech;
use crate::tool::util::{
    append_state, check_track_index, clip_source_rate, flatten_track, load_head_state,
};
use crate::{Tool, ToolContext, ToolResult};

/// How much to drop the music under speech, in dB.
const DEFAULT_DUCK_DB: f32 = -12.0;
/// How long the drop takes. Fast enough to be under the first syllable,
/// slow enough not to click.
const DEFAULT_ATTACK_MS: f32 = 120.0;
/// How long the recovery takes. Slower than the attack, as a hand on a
/// fader would be.
const DEFAULT_RELEASE_MS: f32 = 400.0;
/// How far before a line to start ducking. The thing a sidechain cannot
/// do.
const DEFAULT_PRE_ROLL_MS: f32 = 150.0;
/// Gaps shorter than this are inside one passage of speech rather than
/// between two. Ducking back up for a quarter-second comma is a pump,
/// not an edit.
const DEFAULT_JOIN_GAP_S: f32 = 1.0;

#[derive(Debug, Deserialize)]
struct Args {
    /// The track to duck — the music.
    music_track: usize,
    #[serde(default)]
    duck_db: Option<f32>,
    #[serde(default)]
    attack_ms: Option<f32>,
    #[serde(default)]
    release_ms: Option<f32>,
    #[serde(default)]
    pre_roll_ms: Option<f32>,
    /// Speech gaps shorter than this do not un-duck.
    #[serde(default)]
    join_gap_sec: Option<f32>,
    /// Key on these tracks' audio instead of on the transcript.
    #[serde(default)]
    voice_tracks: Option<Vec<usize>>,
}

pub struct DuckUnderSpeechTool;

impl Tool for DuckUnderSpeechTool {
    fn name(&self) -> &'static str {
        "duck_under_speech"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "duck_under_speech",
            "Drop a music track under the speech and bring it back in the gaps. Keyed on the \
             transcript when there is one; otherwise pass voice_tracks and it keys on where \
             those tracks' audio has voiced (pitched) sound, so a breath, click or hiss does \
             not trigger a duck. Either way it can duck slightly before a line starts, which a \
             live sidechain cannot, and it writes an ordinary volume-automation curve on the \
             music track, so the result is visible and editable rather than a black box.",
            json!({
                "type": "object",
                "properties": {
                    "music_track": { "type": "integer", "description": "Track to duck" },
                    "duck_db": { "type": "number", "description": "How far to drop, in dB. Default -12." },
                    "attack_ms": { "type": "number", "description": "Time to drop. Default 120 ms." },
                    "release_ms": { "type": "number", "description": "Time to recover. Default 400 ms." },
                    "pre_roll_ms": {
                        "type": "number",
                        "description": "Start ducking this long before a line. Default 150 ms.",
                    },
                    "join_gap_sec": {
                        "type": "number",
                        "description": "Speech gaps shorter than this do not un-duck. Default 1 s.",
                    },
                    "voice_tracks": {
                        "type": "array",
                        "items": { "type": "integer", "minimum": 0 },
                        "minItems": 1,
                        "description": "Track(s) with the speech. Key the ducking on where their audio has voiced sound instead of on the transcript; needed when there is no transcript. The voice alone: a track with music in it reads as speech throughout, and loud low rumble, or a fan that runs for part of the track, reads as speech while it lasts.",
                    },
                },
                "required": ["music_track"],
                "additionalProperties": false,
            }),
        )
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };

        let duck_db = args.duck_db.unwrap_or(DEFAULT_DUCK_DB);
        if duck_db > 0.0 {
            return Ok(ToolResult::Error(format!(
                "duck_db is a drop, so it must be negative or zero; got {duck_db}"
            )));
        }
        let attack_s = args.attack_ms.unwrap_or(DEFAULT_ATTACK_MS).max(0.0) / 1000.0;
        let release_s = args.release_ms.unwrap_or(DEFAULT_RELEASE_MS).max(0.0) / 1000.0;
        let pre_roll_s = args.pre_roll_ms.unwrap_or(DEFAULT_PRE_ROLL_MS).max(0.0) / 1000.0;
        let join_gap_s = args.join_gap_sec.unwrap_or(DEFAULT_JOIN_GAP_S).max(0.0);

        let mut state = match load_head_state(ctx) {
            Ok(s) => s,
            Err(msg) => return Ok(ToolResult::Error(msg)),
        };
        if let Err(msg) = check_track_index(&state.tracks, args.music_track) {
            return Ok(ToolResult::Error(msg));
        }
        // Checked before the voice is read: decoding and analysing a
        // long voice track is the expensive part, and it would be thrown
        // away for a music track that has nothing to automate.
        if state.tracks[args.music_track].clips.is_empty() {
            return Ok(ToolResult::Error(format!(
                "track {} has no clips to automate",
                args.music_track
            )));
        }

        // Where the speech is. An explicitly named voice track wins over
        // a transcript: the transcript is session-level and cannot say
        // which track it belongs to, so a track the caller named is the
        // better key.
        //
        // Resolved before `music` is borrowed mutably, because it reads
        // other tracks of the same state.
        let (passages, keyed_on, voice_tracks) = match args.voice_tracks {
            Some(requested) => {
                // First-seen order, so the label and summary read the
                // way the caller wrote them.
                let mut voice_tracks: Vec<usize> = Vec::with_capacity(requested.len());
                for t in requested {
                    if !voice_tracks.contains(&t) {
                        voice_tracks.push(t);
                    }
                }
                if voice_tracks.is_empty() {
                    return Ok(ToolResult::Error(
                        "voice_tracks is empty; name the track(s) with the speech".to_string(),
                    ));
                }
                let mut found = Vec::new();
                for &t in &voice_tracks {
                    if let Err(msg) = check_track_index(&state.tracks, t) {
                        return Ok(ToolResult::Error(msg));
                    }
                    if t == args.music_track {
                        return Ok(ToolResult::Error(format!(
                            "track {t} is the music track; it cannot also be the voice it \
                             ducks under"
                        )));
                    }
                    let clips = &state.tracks[t].clips;
                    if clips.is_empty() {
                        return Ok(ToolResult::Error(format!("voice track {t} has no clips")));
                    }
                    // The timeline as it plays, not the source file:
                    // speech sits where the clip sits, and a cut or a
                    // moved clip changes where. No track gain or effects
                    // are applied, which is right for where speech is.
                    let audio = match flatten_track(clips) {
                        Ok(a) => a,
                        Err(msg) => return Ok(ToolResult::Error(msg)),
                    };
                    // Only the frames a clip sits on are the voice's
                    // audio. The rest of the window is the silence
                    // `flatten_track` lays down before and between
                    // clips, and it must not set the noise floor.
                    let channels = audio.channels as usize;
                    let frames = audio.window.len() / channels.max(1);
                    let covered: Vec<Range<usize>> = clips
                        .iter()
                        .map(|c| {
                            let from = usize::try_from(c.start_in_track).unwrap_or(usize::MAX);
                            let len = usize::try_from(c.length).unwrap_or(usize::MAX);
                            from.min(frames)..from.saturating_add(len).min(frames)
                        })
                        .collect();
                    found.extend(detect_speech(
                        &audio.window,
                        audio.sample_rate,
                        channels,
                        &covered,
                    ));
                }
                let passages = join_passages(found, join_gap_s);
                if passages.is_empty() {
                    return Ok(ToolResult::Error(format!(
                        "found no speech on track(s) {}: nothing voiced stands above that \
                         audio's own noise floor, so there is nothing to duck under",
                        track_list(&voice_tracks)
                    )));
                }
                (passages, "audio", voice_tracks)
            }
            None => {
                let Some(transcript) = state.transcript.clone() else {
                    return Ok(ToolResult::Error(
                        "this session has no transcript; pass `voice_tracks` (the track(s) with \
                         the speech) to key the ducking on that audio instead"
                            .to_string(),
                    ));
                };
                let passages = speech_passages(&transcript.words, join_gap_s);
                if passages.is_empty() {
                    return Ok(ToolResult::Error(
                        "the transcript has no words, so there is no speech to duck under"
                            .to_string(),
                    ));
                }
                (passages, "transcript", Vec::new())
            }
        };
        let speech_sec: f64 = passages.iter().map(|p| (p.end_s - p.start_s) as f64).sum();

        let music = &mut state.tracks[args.music_track];

        // Every clip on the track, not just the first. A track that has
        // been cut or split holds several, and ducking only `clips[0]`
        // would leave the music at full level under every line after
        // the first edit — the failure being silent, since the first
        // half of the track would sound exactly right.
        //
        // Envelope times are relative to each clip's own start, which is
        // what `set_clip_envelope` and the automation lane both use, so
        // each clip gets the passages mapped into its own frame.
        let mut ducks = 0usize;
        let mut clips_touched = 0usize;
        for clip in music.clips.iter_mut() {
            // The clip's own rate, not the session's (#234). Its
            // `start_in_track` and `length` are counted in source
            // frames, and the envelope times this writes are read back
            // in the same domain — so a 44.1 kHz bed in a 48 kHz
            // project had its ducking placed 8.8% off the speech it was
            // ducking under.
            let clip_rate = match clip_source_rate(clip) {
                Ok(r) => r as f64,
                Err(msg) => return Ok(ToolResult::Error(msg)),
            };
            let (points, n) = build_envelope(
                &passages,
                clip.start_in_track as f64 / clip_rate,
                clip.length,
                clip_rate,
                duck_db,
                attack_s,
                release_s,
                pre_roll_s,
            );
            if points.is_empty() {
                continue;
            }
            clip.volume_envelope = points;
            ducks += n;
            clips_touched += 1;
        }

        let plural = if passages.len() == 1 { "" } else { "s" };
        let label = if voice_tracks.is_empty() {
            format!(
                "duck track {} under speech ({} passage{plural})",
                args.music_track,
                passages.len(),
            )
        } else {
            format!(
                "duck track {} under speech on track(s) {} ({} passage{plural})",
                args.music_track,
                track_list(&voice_tracks),
                passages.len(),
            )
        };
        let new_id = match append_state(ctx, state, label) {
            Ok(id) => id,
            Err(msg) => return Ok(ToolResult::Error(msg)),
        };

        let found_in = if voice_tracks.is_empty() {
            "speech".to_string()
        } else {
            format!(
                "speech found in the audio of track(s) {} (voiced sound, so breaths and clicks \
                 are ignored)",
                track_list(&voice_tracks)
            )
        };
        let mut out = json!({
            "node_id": new_id.to_hex(),
            "passages": passages.len(),
            "ducks": ducks,
            "clips": clips_touched,
            "duck_db": duck_db,
            "keyed_on": keyed_on,
            "speech_sec": speech_sec,
            "summary": format!(
                "Ducked track {} by {:.1} dB under {} passage{plural} of {found_in}, starting \
                 {:.0}ms before each line and recovering over {:.0}ms. The result is an \
                 ordinary automation curve — drag it if a duck lands wrong.",
                args.music_track,
                duck_db,
                passages.len(),
                pre_roll_s * 1000.0,
                release_s * 1000.0,
            ),
        });
        if !voice_tracks.is_empty() {
            out["voice_tracks"] = json!(voice_tracks);
        }
        Ok(ToolResult::Ok(out))
    }
}

/// `1, 3` — the track numbers as the caller would write them.
fn track_list(tracks: &[usize]) -> String {
    tracks
        .iter()
        .map(usize::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

/// A stretch of speech, in seconds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Passage {
    pub start_s: f32,
    pub end_s: f32,
}

/// Merge words into passages, joining anything separated by less than
/// `join_gap_s`.
///
/// Ducking back up for the pause between two words in a sentence would
/// be a pump rather than an edit, so only real gaps — where the music
/// has time to be heard — split a passage.
pub fn speech_passages(words: &[session::TranscriptWord], join_gap_s: f32) -> Vec<Passage> {
    let spans = words
        .iter()
        .map(|w| Passage {
            start_s: w.start_s,
            end_s: w.end_s,
        })
        .collect();
    join_passages(spans, join_gap_s)
}

/// Sort spans by start and join any separated by less than `join_gap_s`.
///
/// What [`speech_passages`] does to words, for spans from anywhere: the
/// passages found in several voice tracks' audio arrive as one list per
/// track, not in time order, and two voices overlapping are one passage
/// of speech to duck under. An overlap has a negative gap, so it joins.
pub(crate) fn join_passages(mut spans: Vec<Passage>, join_gap_s: f32) -> Vec<Passage> {
    spans.sort_by(|a, b| a.start_s.total_cmp(&b.start_s));
    let mut out: Vec<Passage> = Vec::new();
    for span in spans {
        match out.last_mut() {
            Some(last) if span.start_s - last.end_s < join_gap_s => {
                last.end_s = last.end_s.max(span.end_s);
            }
            _ => out.push(span),
        }
    }
    out
}

/// Four points per passage: down before the line, hold, and back up
/// after it.
///
/// Times are relative to the clip and are produced in samples, because
/// that is the resolution the envelope actually has and rounding after
/// the fact is what makes coincident points. Passages that fall outside
/// the clip are skipped — a duck with no music under it is not a duck.
///
/// Returns the points and how many passages were actually ducked.
#[allow(clippy::too_many_arguments)]
fn build_envelope(
    passages: &[Passage],
    clip_start_s: f64,
    clip_len: u64,
    sr: f64,
    duck_db: f32,
    attack_s: f32,
    release_s: f32,
    pre_roll_s: f32,
) -> (Vec<session::EnvelopePoint>, usize) {
    let samples = |t: f64| -> u64 { (t * sr).round().max(0.0) as u64 };
    let clip_len_s = clip_len as f64 / sr;

    let mut raw: Vec<(u64, f32)> = Vec::new();
    let mut ducks = 0usize;

    for p in passages {
        // Into clip-relative time.
        let start = p.start_s as f64 - clip_start_s;
        let end = p.end_s as f64 - clip_start_s;
        if end <= 0.0 || start >= clip_len_s {
            continue;
        }

        let duck_at = samples((start - pre_roll_s as f64).max(0.0));
        let full_at = (duck_at + samples(attack_s as f64)).min(clip_len);
        let hold_to = samples(end.max(0.0)).min(clip_len);
        let up_at = (hold_to + samples(release_s as f64)).min(clip_len);

        // A passage that starts before the clip does needs no ramp in.
        if duck_at > 0 {
            raw.push((duck_at, 0.0));
        }
        raw.push((full_at, duck_db));
        raw.push((hold_to, duck_db));
        // `<=`, not `<`. A release ramp that lands exactly on the clip
        // boundary is the common case for a passage near the end, and
        // dropping the point there leaves the last value at `duck_db` —
        // the music stays ducked to the end of the clip instead of
        // recovering by it. A point at `clip_len` is a valid envelope
        // position; the renderer holds the last value forwards anyway.
        if up_at <= clip_len {
            raw.push((up_at, 0.0));
        }
        ducks += 1;
    }

    // Strictly ascending, because the renderer interpolates between
    // neighbours and a pair sharing a time is a division by zero
    // waiting to happen.
    //
    // The interesting case is a zero-length ramp — `attack_ms: 0` asks
    // for a step, and a step is two points one sample apart, not one
    // point with two values. Collapsing them instead (keeping either
    // one) would silently turn "drop instantly at 1.5s" into "ramp down
    // between 1.5s and the end of the line", which is not what was
    // asked for. The sort is stable, so within a time the points are
    // still in the order they were pushed: level first, then the value
    // that governs from there on.
    raw.sort_by_key(|p| p.0);
    let mut out: Vec<session::EnvelopePoint> = Vec::with_capacity(raw.len());
    for (t, gain_db) in raw {
        let time_samples = match out.last() {
            Some(prev) if t <= prev.time_samples => {
                // Redundant: the curve is already at this level here.
                if prev.gain_db == gain_db {
                    continue;
                }
                prev.time_samples + 1
            }
            _ => t,
        };
        if time_samples > clip_len {
            continue;
        }
        out.push(session::EnvelopePoint {
            time_samples,
            gain_db,
        });
    }

    (out, ducks)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span(start_s: f32, end_s: f32) -> Passage {
        Passage { start_s, end_s }
    }

    /// Passages from several voice tracks arrive one list per track.
    /// They have to be put in time order and the overlaps joined, or a
    /// later track's early line is dropped in the middle of the curve.
    #[test]
    fn join_passages_sorts_and_merges_overlaps() {
        let joined = join_passages(vec![span(10.0, 11.0), span(2.0, 3.0), span(2.5, 4.0)], 1.0);
        assert_eq!(joined, vec![span(2.0, 4.0), span(10.0, 11.0)]);
    }

    #[test]
    fn join_passages_joins_a_gap_shorter_than_the_limit_only() {
        assert_eq!(
            join_passages(vec![span(0.0, 1.0), span(1.5, 2.0)], 1.0),
            vec![span(0.0, 2.0)]
        );
        assert_eq!(
            join_passages(vec![span(0.0, 1.0), span(2.0, 3.0)], 1.0),
            vec![span(0.0, 1.0), span(2.0, 3.0)],
            "a gap equal to the limit stays a gap"
        );
    }
}
