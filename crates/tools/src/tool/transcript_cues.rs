//! Transcript cues use the times stored by `transcribe`, without retiming audio.

use serde_json::{json, Value};
use session::{SessionState, Transcript, TranscriptSource, TranscriptWord};

const MAX_WORDS: usize = 200_000;
const MAX_TEXT_BYTES: usize = 4 * 1024 * 1024;
const MAX_CUES: usize = 10_000;

#[derive(Clone, Copy)]
pub(crate) struct Grouping {
    pause_sec: f64,
    max_cue_sec: f64,
    max_chars_per_line: usize,
}

impl Default for Grouping {
    fn default() -> Self {
        Self {
            pause_sec: 0.75,
            max_cue_sec: 6.0,
            max_chars_per_line: 42,
        }
    }
}

impl Grouping {
    pub(crate) fn from_options(
        pause_sec: Option<f64>,
        max_cue_sec: Option<f64>,
        max_chars_per_line: Option<usize>,
    ) -> Result<Self, String> {
        let pause_sec = pause_sec.unwrap_or(Self::default().pause_sec);
        if !pause_sec.is_finite() || pause_sec < 0.0 {
            return Err("pause_sec must be finite and nonnegative".to_string());
        }
        let max_cue_sec = max_cue_sec.unwrap_or(Self::default().max_cue_sec);
        if !max_cue_sec.is_finite() || max_cue_sec <= 0.0 {
            return Err("max_cue_sec must be finite and positive".to_string());
        }
        let max_chars_per_line = max_chars_per_line.unwrap_or(Self::default().max_chars_per_line);
        if !(1..=200).contains(&max_chars_per_line) {
            return Err("max_chars_per_line must be an integer from 1 to 200".to_string());
        }
        Ok(Self {
            pause_sec,
            max_cue_sec,
            max_chars_per_line,
        })
    }
}

#[derive(Clone, Copy)]
pub(crate) enum BoundaryMode {
    Segment,
    Sentence,
}

pub(crate) struct Cue {
    /// Preserve recorded precision for annotations; only captions require ms.
    pub(crate) start_sec: f64,
    pub(crate) start_ms: u64,
    pub(crate) end_ms: u64,
    pub(crate) text: String,
    pub(crate) caption: String,
}

#[derive(Clone, Copy, Default)]
struct Lines {
    count: usize,
    last_width: usize,
}

impl Lines {
    fn append(&mut self, text: &str, width: usize) {
        for part in text.split_whitespace() {
            let length = part.chars().count();
            if self.count == 0 {
                self.count = 1;
            }
            if self.last_width > 0 {
                if self.last_width + 1 + length <= width {
                    self.last_width += 1 + length;
                    continue;
                }
                self.count += 1;
            }
            self.count += (length - 1) / width;
            self.last_width = (length - 1) % width + 1;
        }
    }
}

fn wrap(text: &str, width: usize) -> String {
    let mut output = String::new();
    let mut line_width = 0;
    for part in text.split_whitespace() {
        if line_width > 0 {
            if line_width + 1 + part.chars().count() <= width {
                output.push(' ');
                line_width += 1;
            } else {
                output.push('\n');
                line_width = 0;
            }
        }
        for character in part.chars() {
            if line_width == width {
                output.push('\n');
                line_width = 0;
            }
            output.push(character);
            line_width += 1;
        }
    }
    output
}

/// Milliseconds are rounded once. Reject values a float-to-int cast would clamp.
fn milliseconds(seconds: f64) -> Result<u64, String> {
    let rounded = (seconds * 1000.0).round();
    if !rounded.is_finite() || rounded < 0.0 || rounded >= u64::MAX as f64 {
        return Err("transcript timestamp is outside the supported millisecond range".to_string());
    }
    Ok(rounded as u64)
}

fn sentence_end(text: &str) -> bool {
    text.trim_end()
        .trim_end_matches(['"', '\'', ')', ']', '}', '”', '’', '»'])
        .ends_with(['.', '?', '!', '…'])
}

/// Shared validation happens before either caller writes a file or a node.
fn validate(words: &[TranscriptWord]) -> Result<(), String> {
    if words.is_empty() {
        return Err("transcript has no words; call `transcribe` first".to_string());
    }
    if words.len() > MAX_WORDS {
        return Err(format!("transcript exceeds the {MAX_WORDS} word limit"));
    }
    let mut text_bytes = 0usize;
    let mut previous_start = 0.0f32;
    for (index, word) in words.iter().enumerate() {
        text_bytes = text_bytes
            .checked_add(word.text.len())
            .filter(|&size| size <= MAX_TEXT_BYTES)
            .ok_or_else(|| format!("transcript exceeds the {MAX_TEXT_BYTES} byte text limit"))?;
        if word.text.split_whitespace().next().is_none() {
            return Err(format!("transcript word {index} has empty text"));
        }
        if !word.start_s.is_finite()
            || !word.end_s.is_finite()
            || word.start_s < 0.0
            || word.end_s < word.start_s
            || word.start_s < previous_start
        {
            return Err(format!(
                "transcript word {index} must have finite, nonnegative, chronological times with end >= start"
            ));
        }
        milliseconds(f64::from(word.start_s))?;
        milliseconds(f64::from(word.end_s))?;
        previous_start = word.start_s;
    }
    Ok(())
}

pub(crate) fn cues(
    words: &[TranscriptWord],
    grouping: Grouping,
    mode: BoundaryMode,
) -> Result<Vec<Cue>, String> {
    validate(words)?;
    let mut out = Vec::new();
    let mut start = f64::from(words[0].start_s);
    let mut end = start;
    let mut text = String::new();
    let mut lines = Lines::default();
    for (index, word) in words.iter().enumerate() {
        for part in word.text.split_whitespace() {
            if !text.is_empty() {
                text.push(' ');
            }
            text.push_str(part);
        }
        end = end.max(f64::from(word.end_s));
        lines.append(&word.text, grouping.max_chars_per_line);
        let next = words.get(index + 1);
        let pause = matches!(mode, BoundaryMode::Segment)
            && next.is_some_and(|next| f64::from(next.start_s) - end >= grouping.pause_sec);
        let duration = matches!(mode, BoundaryMode::Segment)
            && next
                .is_some_and(|next| end.max(f64::from(next.end_s)) - start > grouping.max_cue_sec);
        let readability = matches!(mode, BoundaryMode::Segment)
            && next.is_some_and(|next| {
                let mut combined = lines;
                combined.append(&next.text, grouping.max_chars_per_line);
                combined.count > 2
            });
        if next.is_none() || sentence_end(&word.text) || pause || duration || readability {
            if out.len() >= MAX_CUES {
                return Err(format!("transcript exceeds the {MAX_CUES} cue limit"));
            }
            let start_ms = milliseconds(start)?;
            let end_ms = milliseconds(end)?.max(
                start_ms
                    .checked_add(1)
                    .ok_or_else(|| "transcript cue end overflows milliseconds".to_string())?,
            );
            out.push(Cue {
                start_sec: start,
                start_ms,
                end_ms,
                caption: wrap(&text, grouping.max_chars_per_line),
                text: std::mem::take(&mut text),
            });
            lines = Lines::default();
            if let Some(next) = next {
                start = f64::from(next.start_s);
                end = start;
            }
        }
    }
    Ok(out)
}

/// Source identity is separate from timing: later edits cannot be inferred here.
pub(crate) fn source_info(state: &SessionState, transcript: &Transcript) -> Value {
    match &transcript.source {
        Some(TranscriptSource::Track { track }) => {
            let index = state
                .tracks
                .iter()
                .position(|candidate| &candidate.id == track);
            json!({
                "kind": "track",
                "time_basis": "session",
                "stable_track_id": track,
                "current_track_index": index,
                "track_present": index.is_some(),
            })
        }
        Some(TranscriptSource::File) => json!({
            "kind": "file",
            "time_basis": "file",
            "stable_track_id": null,
            "current_track_index": null,
            "track_present": null,
        }),
        None => json!({
            "kind": "unknown",
            "time_basis": "unknown",
            "stable_track_id": null,
            "current_track_index": null,
            "track_present": null,
        }),
    }
}
