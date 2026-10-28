//! Turn stored transcript cues into point markers without changing the audio.

use serde::Deserialize;
use serde_json::{json, Value};
use session::{Annotation, AnnotationId, AnnotationKind, TranscriptSource};

use crate::schema::anthropic_tool;
use crate::tool::transcript_cues::{cues, source_info, BoundaryMode, Grouping};
use crate::tool::util::append_state;
use crate::{Tool, ToolContext, ToolResult};

#[derive(Default, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Mode {
    #[default]
    Segment,
    Sentence,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Args {
    #[serde(default)]
    mode: Mode,
    offset_sec: Option<f64>,
}

pub struct TranscriptToMarkersTool;

impl Tool for TranscriptToMarkersTool {
    fn name(&self) -> &'static str {
        "transcript_to_markers"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            self.name(),
            "Add point markers from the stored transcript's segment or sentence starts, using recorded \
             source times. Appends one session node and preserves existing annotations and audio. \
             File transcripts and transcripts with an unknown source require an explicit \
             offset_sec (zero is allowed) to align source times with the timeline. \
             Run transcribe first if the session has no transcript.",
            json!({
                "type": "object",
                "properties": {
                    "mode": {
                        "type": "string",
                        "enum": ["segment", "sentence"],
                        "default": "segment"
                    },
                    "offset_sec": {
                        "type": "number",
                        "description": "Explicit signed offset in seconds for aligning source times with the timeline; required for file or unknown sources"
                    }
                },
                "additionalProperties": false
            }),
        )
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(args) => args,
            Err(error) => return Ok(ToolResult::Error(format!("invalid arguments: {error}"))),
        };
        if args.offset_sec.is_some_and(|offset| !offset.is_finite()) {
            return Ok(ToolResult::Error("offset_sec must be finite".into()));
        }
        let head = match ctx.store.head() {
            Some(head) => head,
            None => {
                return Ok(ToolResult::Error(
                    "no transcript available; run `transcribe` first".into(),
                ))
            }
        };
        // Loading a head through the audio-edit helper normalizes legacy clip
        // envelopes. A marker-only edit must preserve the stored audio metadata.
        let mut state = match ctx.store.get(head) {
            Ok(node) => node.state,
            Err(error) => {
                return Ok(ToolResult::Error(format!(
                    "failed to read head node: {error}"
                )))
            }
        };
        let transcript = match state.transcript.as_ref() {
            Some(transcript) if !transcript.words.is_empty() => transcript,
            _ => {
                return Ok(ToolResult::Error(
                    "no transcript words available; run `transcribe` first".into(),
                ))
            }
        };
        if let Some(TranscriptSource::Track { track }) = &transcript.source {
            if !state.tracks.iter().any(|candidate| &candidate.id == track) {
                return Ok(ToolResult::Error(
                    "transcript source track was removed; run `transcribe` again for an existing track"
                        .into(),
                ));
            }
        }
        if args.offset_sec.is_none() {
            let source_kind = match &transcript.source {
                Some(TranscriptSource::Track { .. }) => None,
                Some(TranscriptSource::File) => Some("file"),
                None => Some("unknown"),
            };
            if let Some(kind) = source_kind {
                return Ok(ToolResult::Error(format!(
                    "transcript source is {kind}; provide an explicit offset_sec (zero is allowed) \
                     to align source times with the timeline"
                )));
            }
        }
        let source = source_info(&state, transcript);
        let mode = match args.mode {
            Mode::Segment => BoundaryMode::Segment,
            Mode::Sentence => BoundaryMode::Sentence,
        };
        let cues = match cues(&transcript.words, Grouping::default(), mode) {
            Ok(cues) => cues,
            Err(error) => return Ok(ToolResult::Error(error)),
        };
        let marker_count = cues.len();
        let applied_offset_sec = args.offset_sec.unwrap_or(0.0);
        if cues.iter().any(|cue| {
            let time_sec = cue.start_sec + applied_offset_sec;
            !time_sec.is_finite() || time_sec < 0.0
        }) {
            return Ok(ToolResult::Error(
                "offset_sec must place every marker at a finite, nonnegative time".into(),
            ));
        }
        state.annotations.extend(cues.into_iter().map(|cue| {
            Annotation {
                id: AnnotationId::new(),
                name: cue
                    .text
                    .split_whitespace()
                    .take(6)
                    .collect::<Vec<_>>()
                    .join(" ")
                    .chars()
                    .take(80)
                    .collect(),
                kind: AnnotationKind::Marker {
                    time_sec: cue.start_sec + applied_offset_sec,
                },
            }
        }));
        let node_id = match append_state(
            ctx,
            state,
            format!("transcript_to_markers: {marker_count} markers"),
        ) {
            Ok(node_id) => node_id,
            Err(error) => return Ok(ToolResult::Error(error)),
        };
        Ok(ToolResult::Ok(json!({
            "node_id": node_id.to_hex(),
            "marker_count": marker_count,
            "time_basis": source["time_basis"],
            "source": source,
            "applied_offset_sec": applied_offset_sec,
            "summary": format!("Added {marker_count} transcript markers at recorded source times."),
        })))
    }
}
