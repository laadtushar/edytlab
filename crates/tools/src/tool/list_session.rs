//! `list_session` — the tracks, buses or markers of the session, one page
//! at a time (#618).
//!
//! The session block in the system prompt lists the head's tracks, buses
//! and markers, within a byte budget (#573). A session too big for it has
//! each list cut, and the block says how many it left out. Before this
//! tool nothing could list them: `list_tracks` and `list_markers` are
//! Tauri commands for the window, and `export_labels` returned every
//! marker in one result, about 25 KB for a thousand, too big for a model
//! with an 8,192-token context.
//!
//! A page is the lines the block itself uses for that list
//! ([`crate::session_lines`], written once and called by both), so the
//! model reads a track the same way wherever it meets it. It is capped by
//! bytes as well as by count ([`PAGE_MAX_BYTES`], [`PAGE_MAX_ITEMS`]) and
//! says what range it showed of how many and where the next page starts.
//!
//! Read-only, and so never held for approval by Plan first. It runs under
//! the store's lock like the other readers; it reads one node and opens the
//! source of each clip of the tracks on the page, and no more.

use serde::Deserialize;
use serde_json::{json, Value};
use session::SessionState;

use crate::schema::anthropic_tool;
use crate::session_lines::{
    bus_line, marker_line, markers_by_time, session_rate, track_line, BusBrief, TrackBrief,
};
use crate::tool::util::load_head_state;
use crate::{Tool, ToolContext, ToolResult};

/// The most a page may take, in bytes of the result as the model receives
/// it: the JSON of the whole object, which is what the agent loop puts in
/// the conversation. A page stops early when the next line would pass it.
///
/// 4,096 B is about 1,400 tokens at the three bytes per token that
/// `ai/tests/request_size.rs` assumes. An Ollama first request is held to
/// half of an 8,192-token context, and this is a third of the other half,
/// so a page leaves room for the call that asked for it, the model's reply
/// and a second page. A 200-track page and a 1,000-marker page are tested
/// to fit (`ollama_a_page_of_a_huge_session_fits_an_8k_context`).
pub const PAGE_MAX_BYTES: usize = 4_096;

/// The most items a page holds, and how many it holds when `limit` is not
/// given. The byte cap usually ends a page first: a track line is about
/// 90 B and a marker line about 20 B, so this is the bound that matters
/// only for short lines.
pub const PAGE_MAX_ITEMS: usize = 100;

/// What the page keeps free of [`PAGE_MAX_BYTES`] for everything that is not
/// a line: the object's keys, the summary and `next_offset`. At its longest
/// (every number twenty digits) that comes to about 220 B; a test builds it.
const FRAME_BYTES: usize = 256;

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
enum What {
    Tracks,
    Buses,
    Markers,
}

impl What {
    fn as_str(self) -> &'static str {
        match self {
            What::Tracks => "tracks",
            What::Buses => "buses",
            What::Markers => "markers",
        }
    }
}

#[derive(Debug, Deserialize)]
struct Args {
    what: What,
    #[serde(default)]
    offset: usize,
    limit: Option<usize>,
}

pub struct ListSessionTool;

impl Tool for ListSessionTool {
    fn name(&self) -> &'static str {
        "list_session"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "list_session",
            "List tracks, buses or markers a page at a time, in the session block's lines. Read-only.",
            json!({
                "type": "object",
                "properties": {
                    "what": { "type": "string", "enum": ["tracks", "buses", "markers"] },
                    "offset": { "type": "integer", "minimum": 0 },
                    "limit": { "type": "integer", "minimum": 1, "description": "Default and most: 100" }
                },
                "required": ["what"]
            }),
        )
    }

    // Read-only: it reads the head and writes nothing.
    fn mutates(&self) -> bool {
        false
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };
        let state = match load_head_state(ctx) {
            Ok(s) => s,
            Err(e) => return Ok(ToolResult::Error(e)),
        };
        let limit = args
            .limit
            .unwrap_or(PAGE_MAX_ITEMS)
            .clamp(1, PAGE_MAX_ITEMS);
        Ok(ToolResult::Ok(page(&state, args.what, args.offset, limit)))
    }
}

/// The lines of `what` from `offset` on, and how many there are in all.
/// Each line is written when it is asked for, so a page costs the lines it
/// shows (a track's line opens the source of each of its clips), not the
/// whole list.
fn items(
    state: &SessionState,
    what: What,
    offset: usize,
) -> (usize, Box<dyn Iterator<Item = String> + '_>) {
    match what {
        What::Tracks => {
            let rate = session_rate(state);
            (
                state.tracks.len(),
                Box::new(
                    state
                        .tracks
                        .iter()
                        .enumerate()
                        .skip(offset)
                        .map(move |(i, t)| track_line(i, &TrackBrief::of(t, rate))),
                ),
            )
        }
        What::Buses => (
            state.bus_routing.buses.len(),
            Box::new(
                state
                    .bus_routing
                    .buses
                    .iter()
                    .skip(offset)
                    .map(move |b| bus_line(&BusBrief::of(state, b))),
            ),
        ),
        What::Markers => {
            let sorted = markers_by_time(&state.annotations);
            (
                sorted.len(),
                Box::new(sorted.into_iter().skip(offset).map(marker_line)),
            )
        }
    }
}

/// One page: up to `limit` lines of `what` from `offset`, ending early at
/// the byte cap.
fn page(state: &SessionState, what: What, offset: usize, limit: usize) -> Value {
    let (total, lines) = items(state, what, offset);
    let room = PAGE_MAX_BYTES - FRAME_BYTES;
    let mut text = String::new();
    let mut used = 0;
    let mut shown = 0;
    for line in lines.take(limit) {
        let cost = encoded_len(&line);
        if used + cost > room {
            if shown == 0 {
                // One line that is longer than a page. Skipping it would
                // leave the list stuck on it, and a page may not pass the
                // cap, so it is cut and marked.
                text = cut_to(&line, room);
                shown = 1;
            }
            break;
        }
        used += cost;
        text.push_str(&line);
        shown += 1;
    }
    result(what, offset, shown, total, text)
}

/// The result object for a page of `shown` items from `offset`, of `total`
/// in the list, whose lines are `text`. Keys come out in alphabetical
/// order: `next_offset` (absent on the last page), `summary`, `text`.
fn result(what: What, offset: usize, shown: usize, total: usize, text: String) -> Value {
    let name = what.as_str();
    let next = offset.saturating_add(shown);
    let summary = if total == 0 {
        format!("no {name} in the session.")
    } else if shown == 0 {
        format!(
            "offset {offset} is past the end: the session has {total} {name}, at offsets 0-{}.",
            total - 1
        )
    } else {
        let range = if shown == 1 {
            format!("offset {offset}")
        } else {
            format!("offsets {offset}-{}", next - 1)
        };
        let head = format!("{shown} of {total} {name}, {range}.");
        if next < total {
            format!("{head} Next page: offset {next}.")
        } else {
            format!("{head} That is the last page.")
        }
    };
    let mut out = json!({ "summary": summary, "text": text });
    if shown > 0 && next < total {
        out["next_offset"] = json!(next);
    }
    out
}

/// What `text` costs inside a JSON string: its length with the quotes,
/// newlines and the like written as escapes, which is how the model
/// receives it.
fn encoded_len(text: &str) -> usize {
    serde_json::to_string(text).map_or(text.len(), |json| json.len() - 2)
}

/// `line` cut to `room` encoded bytes, ending `…` and a newline.
fn cut_to(line: &str, room: usize) -> String {
    const TAIL: &str = "…\n";
    let mut left = room.saturating_sub(encoded_len(TAIL));
    let mut out = String::new();
    for c in line.trim_end_matches('\n').chars() {
        let cost = encoded_len(c.encode_utf8(&mut [0; 4]));
        if cost > left {
            break;
        }
        left -= cost;
        out.push(c);
    }
    out.push_str(TAIL);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The part of a page that is not lines fits what is kept for it, with
    /// every number as long as a number can be.
    #[test]
    fn the_frame_fits_the_room_kept_for_it_at_its_longest() {
        let at_most = usize::MAX;
        let mut longest = 0;
        for what in [What::Tracks, What::Buses, What::Markers] {
            // A middle page (it names the next offset), the last page,
            // and the page past the end.
            for (offset, shown, total) in [
                (at_most / 2, at_most / 4, at_most),
                (at_most - 2, 2, at_most),
                (at_most, 0, at_most),
            ] {
                let json =
                    serde_json::to_string(&result(what, offset, shown, total, String::new()))
                        .unwrap();
                longest = longest.max(json.len());
            }
        }
        assert!(
            longest <= FRAME_BYTES,
            "the frame is {longest} B, over the {FRAME_BYTES} B kept for it"
        );
    }

    #[test]
    fn a_cut_line_fits_its_room_and_ends_in_the_mark() {
        let line = format!("- {} @ 1.00s\n", "é\"\n".repeat(5_000));
        for room in [10, 100, 3_840] {
            let cut = cut_to(&line, room);
            assert!(
                encoded_len(&cut) <= room,
                "{} over {room}",
                encoded_len(&cut)
            );
            assert!(cut.ends_with("…\n"), "{cut:?}");
        }
        // A room too small for the mark alone still gives the mark, and
        // nothing that is half a character.
        assert_eq!(cut_to(&line, 0), "…\n");
    }

    #[test]
    fn the_encoded_length_is_the_length_in_the_result() {
        let line = "- 0 \"strings 1\": 0.00-1.00s\n";
        let json = serde_json::to_string(&json!({ "text": line })).unwrap();
        assert_eq!(json.len(), r#"{"text":""#.len() + encoded_len(line) + 2);
    }
}
