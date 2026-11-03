//! The agent's marker tools (#766): `find_markers`, `add_markers`,
//! `edit_markers` and `delete_markers`.
//!
//! The agent could place one marker (`label`) and page through them
//! (`list_session`), but renaming, moving, resizing and deleting a marker
//! were window-only (`update_marker`, `remove_marker`). Four tools, not
//! seven: finding is one read-only tool, the three writers each take a
//! list, so a podcast's chapter markers are one call and one undo step.
//!
//! Seconds are session seconds, as on the timeline and in `label`; a marker
//! is stored in seconds, so no sample rate enters (CLAUDE.md's rate rules
//! are about clip frames).
//!
//! **The window and these tools agree** on what a label may be and how an
//! edit is read: `session::check_marker_time`, `check_region` and
//! `AnnotationEdit`, which the window's `add_marker` and `update_marker` call
//! too. A moved region keeps its length
//! ([`session::AnnotationKind::starting_at`]), as it does when it is dragged,
//! and a move that would lose that length (a time so large the end rounds to
//! the start) is refused. This covers the new tools only: `label` has its
//! own, looser rule and still takes a negative time.
//!
//! **A call changes everything or nothing.** The writers check every item
//! before they touch the state, so a call naming an id the head does not
//! hold, or asking for a region that is not one, appends no node. A call
//! that changes nothing appends none either. Otherwise a call appends one
//! node, one step of undo.
//!
//! **Bounded.** `find_markers` pages like `list_session` (the same cap, in
//! bytes and items). The writers take at most [`MAX_BATCH`] items; the
//! interval form of `add_markers` computes its count first and refuses an
//! absurd one before it places any. A name or id a result or a refusal
//! repeats is cut to [`ECHO_CHARS`] characters, because a name the window
//! stored has no limit and one the caller sent may be enormous.

use std::collections::HashSet;

use serde::Deserialize;
use serde_json::{json, Value};
use session::{
    check_marker_time, check_region, Annotation, AnnotationEdit, AnnotationId, AnnotationKind,
    SessionState, MAX_NAME_CHARS,
};
use uuid::Uuid;

use crate::schema::anthropic_tool;
use crate::session_lines::{marker_line_with_id, markers_by_time};
use crate::tool::list_session::{paged, PAGE_MAX_ITEMS};
use crate::tool::util::{append_state, args_and_head, load_head_state, parse_args};
use crate::{Tool, ToolContext, ToolResult};

/// The most items a writer takes in one call, and the most markers the
/// interval form places. A podcast chapter every ten seconds for an hour is
/// 360; a thousand-marker session is why `list_session` pages.
pub const MAX_BATCH: usize = 500;

/// How many names a result repeats back, so a deletion by filter can be
/// checked without listing everything it took.
const NAMES_SHOWN: usize = 10;

/// The most characters of a name or an id a result or a refusal repeats.
const ECHO_CHARS: usize = 60;

/// `text` cut to [`ECHO_CHARS`] characters, with an ellipsis if it was cut.
fn echo(text: &str) -> String {
    let mut chars = text.chars();
    let head: String = chars.by_ref().take(ECHO_CHARS).collect();
    if chars.next().is_some() {
        format!("{head}…")
    } else {
        head
    }
}

fn refuse(message: impl Into<String>) -> crate::Result<ToolResult> {
    Ok(ToolResult::Error(message.into()))
}

// ---------------------------------------------------------------------
// Selecting markers: by name and by time
// ---------------------------------------------------------------------

/// Which markers a call means, by name and by time. An empty selector
/// matches everything, which `find_markers` wants and `delete_markers`
/// refuses.
#[derive(Debug, Default, Deserialize)]
struct Selector {
    name: Option<String>,
    from: Option<f64>,
    to: Option<f64>,
}

/// How a label meets a `from`..`to` range. Finding wants everything that
/// touches it; deleting wants only what lies inside it, because a region
/// that spans the whole piece "overlaps" any range and is not what "delete
/// the markers between 60 and 120" means.
#[derive(Debug, Clone, Copy)]
enum RangeRule {
    Overlaps,
    WhollyInside,
}

impl Selector {
    /// The name to look for, lowercased; an empty name is no name.
    fn needle(&self) -> Option<String> {
        self.name
            .as_deref()
            .map(str::trim)
            .filter(|n| !n.is_empty())
            .map(str::to_lowercase)
    }

    fn is_empty(&self) -> bool {
        self.needle().is_none() && self.from.is_none() && self.to.is_none()
    }

    fn check(&self) -> Result<(), String> {
        for (what, v) in [("from", self.from), ("to", self.to)] {
            if v.is_some_and(|v| !v.is_finite()) {
                return Err(format!("`{what}` must be a finite number of seconds"));
            }
        }
        if let (Some(from), Some(to)) = (self.from, self.to) {
            if from > to {
                return Err(format!("`from` ({from}) must not be after `to` ({to})"));
            }
        }
        Ok(())
    }

    /// The name must contain the needle (any case) and the label must meet
    /// the range by `rule`. Either end of the range may be left open.
    fn matches(&self, a: &Annotation, rule: RangeRule) -> bool {
        if let Some(needle) = self.needle() {
            if !a.name.to_lowercase().contains(&needle) {
                return false;
            }
        }
        let (start, end) = match a.kind {
            AnnotationKind::Marker { time_sec } => (time_sec, time_sec),
            AnnotationKind::Region { start_sec, end_sec } => (start_sec, end_sec),
        };
        match rule {
            RangeRule::Overlaps => {
                self.from.is_none_or(|from| end >= from) && self.to.is_none_or(|to| start <= to)
            }
            RangeRule::WhollyInside => {
                self.from.is_none_or(|from| start >= from) && self.to.is_none_or(|to| end <= to)
            }
        }
    }
}

fn parse_id(id: &str) -> Result<AnnotationId, String> {
    Uuid::parse_str(id).map(AnnotationId).map_err(|_| {
        format!(
            "{:?} is not a marker id; `find_markers` lists them with their ids",
            echo(id)
        )
    })
}

fn find_index(state: &SessionState, id: AnnotationId, text: &str) -> Result<usize, String> {
    state
        .annotations
        .iter()
        .position(|a| a.id == id)
        .ok_or_else(|| {
            format!(
                "no marker has id {}; `find_markers` lists the ids",
                echo(text)
            )
        })
}

fn check_name(name: &str) -> Result<(), String> {
    let len = name.chars().count();
    if len > MAX_NAME_CHARS {
        return Err(format!(
            "a marker name is at most {MAX_NAME_CHARS} characters; this one has {len}"
        ));
    }
    Ok(())
}

/// Up to [`NAMES_SHOWN`] names, each cut to [`ECHO_CHARS`], quoted, and how
/// many more there were.
fn names_text<'a>(names: impl ExactSizeIterator<Item = &'a str>) -> String {
    let total = names.len();
    let shown: Vec<String> = names
        .take(NAMES_SHOWN)
        .map(|n| format!("'{}'", echo(n)))
        .collect();
    let mut text = shown.join(", ");
    if total > NAMES_SHOWN {
        text.push_str(&format!(", and {} more", total - NAMES_SHOWN));
    }
    text
}

fn append(ctx: &mut ToolContext, state: SessionState, label: String) -> Result<String, String> {
    append_state(ctx, state, label).map(|id| id.to_hex())
}

// ---------------------------------------------------------------------
// find_markers
// ---------------------------------------------------------------------

// `deny_unknown_fields` cannot be combined with `flatten`; the schema's
// `additionalProperties: false` refuses a stray key before this is read.
#[derive(Debug, Deserialize)]
struct FindArgs {
    #[serde(flatten)]
    selector: Selector,
    #[serde(default)]
    offset: usize,
    limit: Option<usize>,
}

pub struct FindMarkersTool;

impl Tool for FindMarkersTool {
    fn name(&self) -> &'static str {
        "find_markers"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "find_markers",
            "Find markers/regions by name and/or time range, in time order, with the ids \
             edit_markers and delete_markers take. Read-only; paged.",
            json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Part of a name, any case" },
                    "from": { "type": "number", "description": "Seconds; overlapping regions count" },
                    "to": { "type": "number" },
                    "offset": { "type": "integer" },
                    "limit": { "type": "integer" }
                },
                "additionalProperties": false
            }),
        )
    }

    // Read-only: it reads the head and writes nothing.
    fn mutates(&self) -> bool {
        false
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: FindArgs = match parse_args(args) {
            Ok(a) => a,
            Err(r) => return Ok(r),
        };
        if let Err(e) = args.selector.check() {
            return refuse(e);
        }
        let state = match load_head_state(ctx) {
            Ok(s) => s,
            Err(e) => return refuse(e),
        };
        let matched: Vec<&Annotation> = markers_by_time(&state.annotations)
            .into_iter()
            .filter(|a| args.selector.matches(a, RangeRule::Overlaps))
            .collect();
        let limit = args
            .limit
            .unwrap_or(PAGE_MAX_ITEMS)
            .clamp(1, PAGE_MAX_ITEMS);
        let lines = matched
            .iter()
            .skip(args.offset)
            .map(|a| marker_line_with_id(a));
        Ok(ToolResult::Ok(paged(
            "markers that match",
            Some("no markers match."),
            matched.len(),
            args.offset,
            limit,
            lines,
        )))
    }
}

// ---------------------------------------------------------------------
// add_markers
// ---------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct NewMarker {
    name: String,
    time: Option<f64>,
    start: Option<f64>,
    end: Option<f64>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AddArgs {
    markers: Option<Vec<NewMarker>>,
    every: Option<f64>,
    from: Option<f64>,
    to: Option<f64>,
    name: Option<String>,
}

pub struct AddMarkersTool;

impl AddMarkersTool {
    fn from_list(list: Vec<NewMarker>) -> Result<Vec<Annotation>, String> {
        if list.is_empty() {
            return Err("`markers` is empty; give at least one marker".into());
        }
        if list.len() > MAX_BATCH {
            return Err(format!(
                "that is {} markers; the most one call places is {MAX_BATCH}",
                list.len()
            ));
        }
        list.into_iter()
            .enumerate()
            .map(|(i, m)| {
                let at = |e: String| format!("markers[{i}] ('{}'): {e}", echo(&m.name));
                check_name(&m.name).map_err(at)?;
                let kind = match (m.time, m.start, m.end) {
                    (Some(t), None, None) => {
                        check_marker_time(t).map_err(at)?;
                        AnnotationKind::Marker { time_sec: t }
                    }
                    (None, Some(start_sec), Some(end_sec)) => {
                        check_region(start_sec, end_sec).map_err(at)?;
                        AnnotationKind::Region { start_sec, end_sec }
                    }
                    _ => {
                        return Err(at(
                            "give `time` for a point marker, or both `start` and `end` for a \
                             region"
                                .into(),
                        ))
                    }
                };
                Ok(Annotation {
                    id: AnnotationId::new(),
                    name: m.name,
                    kind,
                })
            })
            .collect()
    }

    /// A marker at `from`, `from + every`, ... up to `to`, each time worked
    /// out as `from + i * every`, so a hundred steps of 0.1 end at 10 and
    /// not at 9.99999999999998.
    fn at_intervals(args: &AddArgs, state: &SessionState) -> Result<Vec<Annotation>, String> {
        let every = args.every.unwrap_or(0.0);
        if !every.is_finite() || every <= 0.0 {
            return Err("`every` must be a positive number of seconds".into());
        }
        let from = args.from.unwrap_or(0.0);
        check_marker_time(from).map_err(|e| format!("`from`: {e}"))?;
        let to = match args.to {
            Some(to) => to,
            None => {
                let length = state.length_samples as f64 / f64::from(state.sample_rate.max(1));
                if length <= 0.0 {
                    return Err(
                        "the session has no audio to take an end from; give `to` in seconds".into(),
                    );
                }
                length
            }
        };
        if !to.is_finite() || to < from {
            return Err(format!(
                "`to` ({to}) must be a number of seconds not before `from` ({from})"
            ));
        }
        // The tolerance keeps a last marker that floating point puts a
        // hair past `to` (10 / 0.1 is exact, 0.3 / 0.1 is not).
        let steps = ((to - from) / every + 1e-9).floor();
        if steps + 1.0 > MAX_BATCH as f64 {
            return Err(format!(
                "`every` {every} s from {from} s to {to} s would place {} markers; the most one \
                 call places is {MAX_BATCH}. Raise `every` or narrow `from` and `to`.",
                if steps + 1.0 > 1e9 {
                    "over a billion".to_string()
                } else {
                    (steps as u64 + 1).to_string()
                }
            ));
        }
        let template = args.name.as_deref().unwrap_or("Marker {n}");
        let count = steps as usize + 1;
        check_name(&template.replace("{n}", &count.to_string()))?;
        Ok((0..count)
            .map(|i| Annotation {
                id: AnnotationId::new(),
                name: template.replace("{n}", &(i + 1).to_string()),
                kind: AnnotationKind::Marker {
                    time_sec: (from + i as f64 * every).min(to),
                },
            })
            .collect())
    }
}

impl Tool for AddMarkersTool {
    fn name(&self) -> &'static str {
        "add_markers"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "add_markers",
            "Add markers/regions: `markers`, or one every `every` s from `from` (0) up to and \
             including `to` (session end), named `name` with {n}=1,2,... Max 500.",
            json!({
                "type": "object",
                "properties": {
                    "markers": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "name": { "type": "string" },
                                "time": { "type": "number", "description": "Seconds" },
                                "start": { "type": "number" },
                                "end": { "type": "number" }
                            },
                            "required": ["name"]
                        }
                    },
                    "every": { "type": "number" },
                    "from": { "type": "number" },
                    "to": { "type": "number" },
                    "name": { "type": "string" }
                },
                "additionalProperties": false
            }),
        )
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let (args, mut state) = match args_and_head::<AddArgs>(args, ctx) {
            Ok(v) => v,
            Err(r) => return Ok(r),
        };
        let new = if args.markers.is_some() && args.every.is_some() {
            return refuse("give `markers` or `every`, not both");
        } else if let Some(list) = args.markers {
            if args.from.is_some() || args.to.is_some() || args.name.is_some() {
                return refuse(
                    "`from`, `to` and `name` go with `every`, not with `markers`: a list names \
                     each marker itself. Drop them, or drop `markers` and give `every`",
                );
            }
            Self::from_list(list)
        } else if args.every.is_some() {
            Self::at_intervals(&args, &state)
        } else {
            return refuse(
                "give `markers` (a list) or `every` (seconds between markers); neither was",
            );
        };
        let new = match new {
            Ok(n) => n,
            Err(e) => return refuse(e),
        };
        let added = new.len();
        let names = names_text(new.iter().map(|a| a.name.as_str()));
        state.annotations.extend(new);
        let node_id = match append(ctx, state, format!("add_markers {added}")) {
            Ok(id) => id,
            Err(e) => return refuse(e),
        };
        Ok(ToolResult::Ok(json!({
            "node_id": node_id,
            "added": added,
            "summary": format!("added {added} marker(s): {names}; new head {node_id}"),
        })))
    }
}

// ---------------------------------------------------------------------
// edit_markers
// ---------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EditItem {
    id: String,
    name: Option<String>,
    time: Option<f64>,
    start: Option<f64>,
    end: Option<f64>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EditArgs {
    edits: Vec<EditItem>,
}

pub struct EditMarkersTool;

impl Tool for EditMarkersTool {
    fn name(&self) -> &'static str {
        "edit_markers"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "edit_markers",
            "Rename, move or resize markers by id (find_markers). `time` moves (a region keeps \
             its length); `start`/`end` set a region's edges. All or nothing.",
            json!({
                "type": "object",
                "properties": {
                    "edits": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "id": { "type": "string" },
                                "name": { "type": "string" },
                                "time": { "type": "number" },
                                "start": { "type": "number" },
                                "end": { "type": "number" }
                            },
                            "required": ["id"]
                        }
                    }
                },
                "required": ["edits"],
                "additionalProperties": false
            }),
        )
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: EditArgs = match parse_args(args) {
            Ok(a) => a,
            Err(r) => return Ok(r),
        };
        if args.edits.is_empty() {
            return refuse("`edits` is empty; give at least one {id, ...}");
        }
        if args.edits.len() > MAX_BATCH {
            return refuse(format!(
                "that is {} edits; the most one call takes is {MAX_BATCH}",
                args.edits.len()
            ));
        }
        let mut state = match load_head_state(ctx) {
            Ok(s) => s,
            Err(e) => return refuse(e),
        };
        let before = state.annotations.clone();
        let mut seen = HashSet::new();
        for (i, item) in args.edits.into_iter().enumerate() {
            let at = |e: String| format!("edits[{i}] (id {}): {e}", echo(&item.id));
            let id = match parse_id(&item.id) {
                Ok(id) => id,
                Err(e) => return refuse(format!("edits[{i}]: {e}")),
            };
            if !seen.insert(id) {
                return refuse(at(
                    "this id is edited twice in one call; combine the edits".into()
                ));
            }
            let index = match find_index(&state, id, &item.id) {
                Ok(index) => index,
                Err(e) => return refuse(format!("edits[{i}]: {e}")),
            };
            let edit = AnnotationEdit {
                name: item.name,
                time: item.time,
                start: item.start,
                end: item.end,
            };
            if edit.is_empty() {
                return refuse(at(
                    "nothing to change; give `name`, `time`, or `start` and/or `end`".into(),
                ));
            }
            if let Some(name) = &edit.name {
                if let Err(e) = check_name(name) {
                    return refuse(at(e));
                }
            }
            let target = &mut state.annotations[index];
            let kind = match edit.resolve(&target.kind) {
                Ok(kind) => kind,
                Err(e) => return refuse(at(e)),
            };
            if let Some(name) = edit.name {
                target.name = name;
            }
            if let Some(kind) = kind {
                target.kind = kind;
            }
        }

        let changed = state
            .annotations
            .iter()
            .zip(&before)
            .filter(|(a, b)| a != b)
            .count();
        if changed == 0 {
            return Ok(ToolResult::Ok(json!({
                "edited": 0,
                "summary": "nothing changed: every marker already had those values",
            })));
        }
        let node_id = match append(ctx, state, format!("edit_markers {changed}")) {
            Ok(id) => id,
            Err(e) => return refuse(e),
        };
        Ok(ToolResult::Ok(json!({
            "node_id": node_id,
            "edited": changed,
            "summary": format!("edited {changed} marker(s); new head {node_id}"),
        })))
    }
}

// ---------------------------------------------------------------------
// delete_markers
// ---------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct DeleteArgs {
    ids: Option<Vec<String>>,
    #[serde(flatten)]
    selector: Selector,
}

pub struct DeleteMarkersTool;

impl Tool for DeleteMarkersTool {
    fn name(&self) -> &'static str {
        "delete_markers"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "delete_markers",
            "Delete markers by `ids` (find_markers) or by filter, not both: `name` is part of a \
             name, any case; `from`/`to` take only labels wholly inside that range. Undo \
             restores them.",
            json!({
                "type": "object",
                "properties": {
                    "ids": { "type": "array", "items": { "type": "string" } },
                    "name": { "type": "string" },
                    "from": { "type": "number" },
                    "to": { "type": "number" }
                },
                "additionalProperties": false
            }),
        )
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: DeleteArgs = match parse_args(args) {
            Ok(a) => a,
            Err(r) => return Ok(r),
        };
        if let Err(e) = args.selector.check() {
            return refuse(e);
        }
        let mut state = match load_head_state(ctx) {
            Ok(s) => s,
            Err(e) => return refuse(e),
        };

        let doomed: HashSet<AnnotationId> = match args.ids {
            Some(ids) => {
                if !args.selector.is_empty() {
                    return refuse(
                        "give `ids` or a filter (`name`, `from`, `to`), not both: say which \
                         markers you mean",
                    );
                }
                if ids.is_empty() {
                    return refuse("`ids` is empty; give at least one id from find_markers");
                }
                if ids.len() > MAX_BATCH {
                    return refuse(format!(
                        "that is {} ids; the most one call takes is {MAX_BATCH}",
                        ids.len()
                    ));
                }
                let mut set = HashSet::new();
                for text in &ids {
                    let id = match parse_id(text) {
                        Ok(id) => id,
                        Err(e) => return refuse(e),
                    };
                    if let Err(e) = find_index(&state, id, text) {
                        return refuse(e);
                    }
                    set.insert(id);
                }
                set
            }
            None => {
                if args.selector.is_empty() {
                    return refuse(
                        "say what to delete: `ids` from find_markers, or a filter (`name`, \
                         `from`, `to`). Nothing was deleted.",
                    );
                }
                state
                    .annotations
                    .iter()
                    .filter(|a| args.selector.matches(a, RangeRule::WhollyInside))
                    .map(|a| a.id)
                    .collect()
            }
        };

        if doomed.is_empty() {
            return Ok(ToolResult::Ok(json!({
                "deleted": 0,
                "summary": "no markers matched; nothing was deleted",
            })));
        }
        let names = names_text(
            state
                .annotations
                .iter()
                .filter(|a| doomed.contains(&a.id))
                .map(|a| a.name.as_str())
                .collect::<Vec<_>>()
                .into_iter(),
        );
        state.annotations.retain(|a| !doomed.contains(&a.id));
        let deleted = doomed.len();
        let node_id = match append(ctx, state, format!("delete_markers {deleted}")) {
            Ok(id) => id,
            Err(e) => return refuse(e),
        };
        Ok(ToolResult::Ok(json!({
            "node_id": node_id,
            "deleted": deleted,
            "summary": format!("deleted {deleted} marker(s): {names}; new head {node_id}"),
        })))
    }
}
