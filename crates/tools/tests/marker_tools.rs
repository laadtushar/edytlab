//! The marker tools (#766, scope item 1): `find_markers`, `edit_markers`,
//! `delete_markers` and `add_markers`.
//!
//! Before them the agent could place a marker (`label`) and page through
//! them (`list_session`), but renaming, moving, resizing and deleting one
//! were window-only (`update_marker`, `remove_marker`). These tests go
//! through the dispatcher, as the model does, over a real store.
//!
//! What they pin:
//!
//! - `find_markers` is read-only, bounded by bytes and items, and returns
//!   the ids the editors take;
//! - an editor appends one node, so one undo step takes a whole call back,
//!   and a call that names an id the head does not hold, or asks for
//!   something that cannot be, changes nothing at all;
//! - the tools and the window agree: an edit made through the tool leaves
//!   the annotation the store's own `update_annotation` leaves, which is
//!   what the window's command calls;
//! - the interval form of `add_markers` places `from + i * every`, never a
//!   running sum, and refuses a count no one asked for.

#[path = "support/harness.rs"]
mod harness;

use harness::{marker, region, Session};
use serde_json::{json, Value};
use session::{Annotation, AnnotationId, AnnotationKind};
use tools::ToolDispatcher;

type Fixture = Session;

impl Session {
    fn with(annotations: Vec<Annotation>) -> Self {
        Self::with_annotations(annotations, 0)
    }

    fn with_length(annotations: Vec<Annotation>, length_samples: u64) -> Self {
        Self::with_annotations(annotations, length_samples)
    }

    /// The error text of a call that must be refused.
    fn refused(&mut self, tool: &str, args: Value) -> String {
        self.err(tool, args)
    }

    fn annotations(&self) -> Vec<Annotation> {
        self.store.annotations_for(self.head()).expect("head node")
    }

    fn by_name(&self, name: &str) -> Annotation {
        self.annotations()
            .into_iter()
            .find(|a| a.name == name)
            .unwrap_or_else(|| panic!("no marker named {name:?}"))
    }

    fn id_of(&self, name: &str) -> String {
        self.by_name(name).id.0.to_string()
    }

    /// Run `f` and assert it appended exactly one node whose parent is the
    /// head it started from (so one undo step takes it back).
    fn one_node<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        let before = self.head();
        let r = f(self);
        let after = self.head();
        assert_ne!(before, after, "no node was appended");
        let node = self.store.get(after).expect("new head");
        assert_eq!(
            node.parent,
            Some(before),
            "the new node does not extend the old head"
        );
        r
    }

    /// Run `f` and assert the head did not move.
    fn no_node<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        let before = self.head();
        let r = f(self);
        assert_eq!(self.head(), before, "a node was appended");
        r
    }
}

fn kind_of(f: &Fixture, name: &str) -> AnnotationKind {
    f.by_name(name).kind
}

// ---------------------------------------------------------------------
// find_markers
// ---------------------------------------------------------------------

#[test]
fn find_markers_is_read_only_and_appends_nothing() {
    let d = ToolDispatcher::default_dispatcher();
    assert!(!d.get("find_markers").expect("registered").mutates());
    for editor in ["add_markers", "edit_markers", "delete_markers"] {
        assert!(
            d.get(editor).expect("registered").mutates(),
            "{editor} changes the session and must be held by Plan first"
        );
    }

    let mut f = Fixture::with(vec![marker("intro", 1.0), marker("outro", 9.0)]);
    f.no_node(|f| f.ok("find_markers", json!({ "name": "intro" })));
}

#[test]
fn find_markers_matches_a_name_without_regard_to_case() {
    let mut f = Fixture::with(vec![
        marker("Chorus 1", 10.0),
        marker("verse", 2.0),
        region("CHORUS 2", 30.0, 40.0),
    ]);
    let v = f.ok("find_markers", json!({ "name": "chorus" }));
    let text = v["text"].as_str().unwrap();
    assert!(text.contains("Chorus 1"), "{text}");
    assert!(text.contains("CHORUS 2"), "{text}");
    assert!(!text.contains("verse"), "{text}");
    assert!(v["summary"].as_str().unwrap().contains("2 of 2"), "{v}");
}

#[test]
fn find_markers_matches_a_time_range_including_a_region_that_overlaps_it() {
    let mut f = Fixture::with(vec![
        marker("before", 4.0),
        marker("edge-in", 5.0),
        marker("inside", 7.0),
        marker("edge-out", 10.0),
        marker("after", 11.0),
        region("straddles-start", 3.0, 6.0),
        region("straddles-end", 9.0, 20.0),
        region("covers", 0.0, 50.0),
        region("ends-before", 1.0, 4.9),
        region("starts-after", 10.1, 12.0),
    ]);
    let v = f.ok("find_markers", json!({ "from": 5.0, "to": 10.0 }));
    let text = v["text"].as_str().unwrap();
    for wanted in [
        "edge-in",
        "inside",
        "edge-out",
        "straddles-start",
        "straddles-end",
        "covers",
    ] {
        assert!(text.contains(wanted), "{wanted} missing from:\n{text}");
    }
    for unwanted in ["before", "after", "ends-before", "starts-after"] {
        assert!(!text.contains(unwanted), "{unwanted} in:\n{text}");
    }
    assert!(v["summary"].as_str().unwrap().contains("6 of 6"), "{v}");
}

#[test]
fn find_markers_combines_name_and_range_and_lists_in_time_order() {
    let mut f = Fixture::with(vec![
        marker("cue c", 30.0),
        marker("cue a", 10.0),
        marker("other", 20.0),
        marker("cue b", 20.0),
    ]);
    let v = f.ok("find_markers", json!({ "name": "cue", "from": 15.0 }));
    let text = v["text"].as_str().unwrap();
    assert_eq!(text.lines().count(), 2, "{text}");
    let b = text.find("cue b").unwrap();
    let c = text.find("cue c").unwrap();
    assert!(b < c, "not in time order:\n{text}");
}

#[test]
fn find_markers_returns_the_ids_the_editors_take() {
    let mut f = Fixture::with(vec![marker("intro", 1.0), marker("outro", 9.0)]);
    let id = f.id_of("outro");
    let v = f.ok("find_markers", json!({ "name": "outro" }));
    assert!(v["text"].as_str().unwrap().contains(&id), "{v}");
}

#[test]
fn find_markers_says_so_when_nothing_matches() {
    let mut f = Fixture::with(vec![marker("intro", 1.0)]);
    let v = f.ok("find_markers", json!({ "name": "zzz" }));
    assert!(v["summary"].as_str().unwrap().contains("no markers"), "{v}");
    assert!(v["text"].as_str().unwrap().is_empty(), "{v}");
}

#[test]
fn find_markers_refuses_a_backwards_range() {
    let mut f = Fixture::with(vec![marker("intro", 1.0)]);
    let e = f.refused("find_markers", json!({ "from": 9.0, "to": 2.0 }));
    assert!(e.contains("from") && e.contains("to"), "{e}");
}

/// A thousand markers do not come back in one result: a page is capped by
/// bytes, says how many there are, and `next_offset` reaches the rest.
#[test]
fn find_markers_output_is_bounded_and_pages_reach_every_match() {
    let many: Vec<Annotation> = (0..1_000)
        .map(|i| marker(&format!("cue {i}"), i as f64))
        .collect();
    let mut f = Fixture::with(many);

    let mut seen = 0;
    let mut offset = 0;
    let mut pages = 0;
    loop {
        let v = f.ok("find_markers", json!({ "name": "cue", "offset": offset }));
        let bytes = serde_json::to_string(&v).unwrap().len();
        assert!(bytes <= 4_096, "a page is {bytes} B, over the 4,096 B cap");
        assert!(
            v["summary"].as_str().unwrap().contains("of 1000 markers"),
            "{v}"
        );
        seen += v["text"].as_str().unwrap().lines().count();
        pages += 1;
        assert!(pages < 200, "the pages never end");
        match v.get("next_offset") {
            Some(n) => {
                let n = n.as_u64().unwrap() as usize;
                assert!(n > offset, "next_offset does not advance");
                offset = n;
            }
            None => break,
        }
    }
    assert_eq!(seen, 1_000);
    assert!(pages > 1, "a thousand markers fit one page");
}

#[test]
fn find_markers_honours_a_limit_and_one_enormous_name_still_fits_a_page() {
    let mut f = Fixture::with(vec![
        marker(&"x".repeat(20_000), 1.0),
        marker("b", 2.0),
        marker("c", 3.0),
    ]);
    let v = f.ok("find_markers", json!({ "limit": 1 }));
    assert_eq!(v["text"].as_str().unwrap().lines().count(), 1);
    assert!(serde_json::to_string(&v).unwrap().len() <= 4_096);
    assert_eq!(v["next_offset"], 1);
}

// ---------------------------------------------------------------------
// edit_markers
// ---------------------------------------------------------------------

#[test]
fn edit_markers_renames_moves_and_resizes_in_one_node() {
    let mut f = Fixture::with(vec![
        marker("a", 1.0),
        marker("b", 2.0),
        region("r", 10.0, 14.0),
        region("s", 20.0, 24.0),
        marker("untouched", 5.0),
    ]);
    let (a, b, r, s) = (f.id_of("a"), f.id_of("b"), f.id_of("r"), f.id_of("s"));
    f.one_node(|f| {
        f.ok(
            "edit_markers",
            json!({ "edits": [
                { "id": a, "name": "Intro" },
                { "id": b, "time": 3.5 },
                { "id": r, "time": 30.0 },
                { "id": s, "end": 28.0 },
            ]}),
        )
    });
    assert_eq!(
        f.by_name("Intro").kind,
        AnnotationKind::Marker { time_sec: 1.0 }
    );
    assert_eq!(kind_of(&f, "b"), AnnotationKind::Marker { time_sec: 3.5 });
    // A region that is moved keeps its length.
    assert_eq!(
        kind_of(&f, "r"),
        AnnotationKind::Region {
            start_sec: 30.0,
            end_sec: 34.0
        }
    );
    // Resizing by one edge leaves the other where it was.
    assert_eq!(
        kind_of(&f, "s"),
        AnnotationKind::Region {
            start_sec: 20.0,
            end_sec: 28.0
        }
    );
    assert_eq!(
        kind_of(&f, "untouched"),
        AnnotationKind::Marker { time_sec: 5.0 }
    );
    assert_eq!(f.annotations().len(), 5);
}

#[test]
fn edit_markers_can_set_both_edges_and_turn_a_marker_into_a_region() {
    let mut f = Fixture::with(vec![marker("m", 4.0), region("r", 1.0, 2.0)]);
    let (m, r) = (f.id_of("m"), f.id_of("r"));
    f.ok(
        "edit_markers",
        json!({ "edits": [
            { "id": m, "start": 4.0, "end": 9.0 },
            { "id": r, "start": 0.5, "end": 8.0, "name": "Wide" },
        ]}),
    );
    assert_eq!(
        kind_of(&f, "m"),
        AnnotationKind::Region {
            start_sec: 4.0,
            end_sec: 9.0
        }
    );
    assert_eq!(
        f.by_name("Wide").kind,
        AnnotationKind::Region {
            start_sec: 0.5,
            end_sec: 8.0
        }
    );
}

#[test]
fn edit_markers_refuses_a_missing_id_and_changes_nothing() {
    let mut f = Fixture::with(vec![marker("a", 1.0), marker("b", 2.0)]);
    let a = f.id_of("a");
    let missing = AnnotationId::new().0.to_string();
    let before = f.annotations();
    let e = f.no_node(|f| {
        // The good edit comes first: a refusal must not apply it.
        f.refused(
            "edit_markers",
            json!({ "edits": [
                { "id": a, "name": "renamed" },
                { "id": missing, "name": "ghost" },
            ]}),
        )
    });
    assert!(e.contains(&missing), "{e}");
    assert!(
        e.contains("find_markers"),
        "the refusal says how to find ids: {e}"
    );
    assert_eq!(f.annotations(), before);
}

#[test]
fn edit_markers_refuses_what_cannot_be_and_changes_nothing() {
    let mut f = Fixture::with(vec![marker("m", 4.0), region("r", 10.0, 14.0)]);
    let (m, r) = (f.id_of("m"), f.id_of("r"));
    let before = f.annotations();
    let cases = [
        (
            "not a uuid",
            json!({ "edits": [{ "id": "nope", "name": "x" }] }),
        ),
        (
            "negative time",
            json!({ "edits": [{ "id": m, "time": -1.0 }] }),
        ),
        (
            "time with a region's edges",
            json!({ "edits": [{ "id": r, "time": 2.0, "end": 20.0 }] }),
        ),
        (
            "end before start",
            json!({ "edits": [{ "id": r, "start": 12.0, "end": 11.0 }] }),
        ),
        (
            "a start past the stored end",
            json!({ "edits": [{ "id": r, "start": 15.0 }] }),
        ),
        (
            "a lone edge on a point marker",
            json!({ "edits": [{ "id": m, "end": 9.0 }] }),
        ),
        ("nothing to change", json!({ "edits": [{ "id": m }] })),
        (
            "the same id twice",
            json!({ "edits": [{ "id": m, "name": "a" }, { "id": m, "name": "b" }] }),
        ),
        ("no edits", json!({ "edits": [] })),
    ];
    for (why, args) in cases {
        f.no_node(|f| f.refused("edit_markers", args));
        assert_eq!(f.annotations(), before, "{why}");
    }
}

#[test]
fn edit_markers_that_change_nothing_append_no_node() {
    let mut f = Fixture::with(vec![marker("m", 4.0), region("r", 1.0, 3.0)]);
    let (m, r) = (f.id_of("m"), f.id_of("r"));
    f.no_node(|f| {
        f.ok(
            "edit_markers",
            json!({ "edits": [
                { "id": m, "name": "m", "time": 4.0 },
                { "id": r, "time": 1.0 },
            ]}),
        )
    });
}

/// The window's `update_marker` calls `Store::update_annotation` with the
/// kind `AnnotationEdit::resolve` makes. The tool must leave the same
/// annotation, edit for edit.
#[test]
fn the_tool_and_the_windows_store_call_agree() {
    let start = vec![marker("m", 4.0), region("r", 10.0, 14.0)];
    let edits = |m: &str, r: &str| {
        vec![
            json!({ "id": m, "time": 9.0, "name": "moved" }),
            json!({ "id": r, "time": 0.0 }),
        ]
    };

    let mut by_tool = Fixture::with(start.clone());
    let (m, r) = (by_tool.id_of("m"), by_tool.id_of("r"));
    by_tool.ok("edit_markers", json!({ "edits": edits(&m, &r) }));

    // The window's path: the store call, one label at a time.
    let mut by_window = Fixture::with(start);
    let mut head = by_window.head();
    for a in by_window.annotations() {
        let (name, kind) = match a.name.as_str() {
            "m" => (Some("moved".to_string()), a.kind.starting_at(9.0)),
            _ => (None, a.kind.starting_at(0.0)),
        };
        head = by_window
            .store
            .update_annotation(head, a.id, name, Some(kind))
            .expect("update");
    }
    let window_side = by_window.store.annotations_for(head).unwrap();

    let strip = |v: Vec<Annotation>| -> Vec<(String, AnnotationKind)> {
        v.into_iter().map(|a| (a.name, a.kind)).collect()
    };
    assert_eq!(strip(by_tool.annotations()), strip(window_side));
}

// ---------------------------------------------------------------------
// delete_markers
// ---------------------------------------------------------------------

#[test]
fn delete_markers_by_id_in_one_node() {
    let mut f = Fixture::with(vec![marker("a", 1.0), marker("b", 2.0), marker("c", 3.0)]);
    let (a, c) = (f.id_of("a"), f.id_of("c"));
    let v = f.one_node(|f| f.ok("delete_markers", json!({ "ids": [a, c] })));
    assert_eq!(v["deleted"], 2);
    let left = f.annotations();
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].name, "b");
}

#[test]
fn delete_markers_refuses_a_missing_id_and_deletes_nothing() {
    let mut f = Fixture::with(vec![marker("a", 1.0), marker("b", 2.0)]);
    let a = f.id_of("a");
    let missing = AnnotationId::new().0.to_string();
    let before = f.annotations();
    let e = f.no_node(|f| f.refused("delete_markers", json!({ "ids": [a, missing] })));
    assert!(e.contains(&missing), "{e}");
    assert_eq!(f.annotations(), before);
    f.no_node(|f| f.refused("delete_markers", json!({ "ids": ["not-a-uuid"] })));
}

#[test]
fn delete_markers_by_filter_name_and_range() {
    let mut f = Fixture::with(vec![
        marker("take 1", 1.0),
        marker("take 2", 5.0),
        marker("take 3", 20.0),
        marker("keep", 6.0),
        region("take 4", 4.0, 7.0),
    ]);
    let v = f.one_node(|f| {
        f.ok(
            "delete_markers",
            json!({ "name": "take", "from": 4.0, "to": 10.0 }),
        )
    });
    // take 2 (5.0) and the region take 4 (4..7); take 1 is before, take 3
    // after, and "keep" does not match the name.
    assert_eq!(v["deleted"], 2);
    let mut names: Vec<String> = f.annotations().into_iter().map(|a| a.name).collect();
    names.sort();
    assert_eq!(names, ["keep", "take 1", "take 3"]);
}

#[test]
fn delete_markers_by_filter_that_matches_nothing_appends_no_node() {
    let mut f = Fixture::with(vec![marker("a", 1.0)]);
    let v = f.no_node(|f| f.ok("delete_markers", json!({ "name": "zzz" })));
    assert_eq!(v["deleted"], 0);
}

#[test]
fn delete_markers_will_not_guess_what_to_delete() {
    let mut f = Fixture::with(vec![marker("a", 1.0), marker("b", 2.0)]);
    let a = f.id_of("a");
    // No selector at all is not "everything".
    f.no_node(|f| f.refused("delete_markers", json!({})));
    f.no_node(|f| f.refused("delete_markers", json!({ "ids": [] })));
    // Ids and a filter together are ambiguous.
    f.no_node(|f| f.refused("delete_markers", json!({ "ids": [a], "name": "a" })));
    f.no_node(|f| f.refused("delete_markers", json!({ "from": 5.0, "to": 1.0 })));
    assert_eq!(f.annotations().len(), 2);
}

#[test]
fn deleting_is_one_undo_step() {
    let mut f = Fixture::with(vec![marker("a", 1.0), marker("b", 2.0), marker("c", 3.0)]);
    let before = f.head();
    f.ok("delete_markers", json!({ "name": "" , "from": 0.0 }));
    assert!(f.annotations().is_empty());
    let node = f.store.get(f.head()).unwrap();
    assert_eq!(node.parent, Some(before), "undo goes back to the markers");
    assert_eq!(f.store.annotations_for(before).unwrap().len(), 3);
}

/// Review fix 3: `find_markers` reads a range as "overlaps it", so a
/// region that spans the whole piece is found by `from 60 to 120`. A
/// deletion must not take it: only labels wholly inside the range go.
#[test]
fn delete_markers_by_range_takes_only_labels_wholly_inside_it() {
    let mut f = Fixture::with(vec![
        marker("before", 30.0),
        marker("inside", 90.0),
        marker("on the edge", 120.0),
        region("inside region", 70.0, 100.0),
        region("whole piece", 0.0, 3_600.0),
        region("sticks out", 100.0, 130.0),
        marker("after", 200.0),
    ]);
    // The overlap rule finds the big regions; that is find's rule.
    let found = f.ok("find_markers", json!({ "from": 60.0, "to": 120.0 }));
    assert!(found["text"].to_string().contains("whole piece"), "{found}");
    let v = f.one_node(|f| f.ok("delete_markers", json!({ "from": 60.0, "to": 120.0 })));
    assert_eq!(v["deleted"], 3, "{v}");
    let mut names: Vec<String> = f.annotations().into_iter().map(|a| a.name).collect();
    names.sort();
    assert_eq!(names, ["after", "before", "sticks out", "whole piece"]);
}

#[test]
fn delete_markers_by_one_open_ended_bound_is_wholly_inside_it() {
    let mut f = Fixture::with(vec![
        marker("early", 5.0),
        region("spans 50", 40.0, 60.0),
        region("late", 55.0, 70.0),
    ]);
    // `from` alone: starts at or after 50.
    f.ok("delete_markers", json!({ "from": 50.0 }));
    let mut names: Vec<String> = f.annotations().into_iter().map(|a| a.name).collect();
    names.sort();
    assert_eq!(names, ["early", "spans 50"]);
    // `to` alone: ends at or before 10.
    f.ok("delete_markers", json!({ "to": 10.0 }));
    assert_eq!(f.annotations().len(), 1);
    assert_eq!(f.annotations()[0].name, "spans 50");
}

#[test]
fn delete_markers_name_is_part_of_a_name_and_the_schema_says_so() {
    let mut f = Fixture::with(vec![marker("Intro", 1.0), marker("Introduction", 2.0)]);
    let schema = ToolDispatcher::default_dispatcher().tool_schemas();
    let tool = schema
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "delete_markers")
        .expect("delete_markers");
    let text = tool.to_string().to_lowercase();
    assert!(
        text.contains("part of a name"),
        "the schema must say `name` is a substring match: {text}"
    );
    assert!(
        text.contains("wholly inside"),
        "the schema must say a range takes only what is inside it: {text}"
    );
    let v = f.ok("delete_markers", json!({ "name": "intro" }));
    assert_eq!(v["deleted"], 2);
}

/// Review fix 2: a name the window stored has no length limit, so what a
/// deletion repeats back must be cut, or ten 20 KB names are a 200 KB answer.
#[test]
fn delete_markers_result_stays_small_when_the_names_are_enormous() {
    let big = |i: usize| marker(&format!("take {i} {}", "x".repeat(20_000)), i as f64);
    let mut f = Fixture::with((0..12).map(big).collect());
    let v = f.one_node(|f| f.ok("delete_markers", json!({ "name": "take" })));
    assert_eq!(v["deleted"], 12);
    let size = v.to_string().len();
    assert!(size < 2_000, "the answer is {size} B");
    let summary = v["summary"].as_str().unwrap();
    assert!(summary.contains("and 2 more"), "{summary}");
    assert!(
        summary.contains('…'),
        "a cut name ends in an ellipsis: {summary}"
    );
    assert!(
        summary.contains("take 0 "),
        "the start of each name is kept: {summary}"
    );
}

#[test]
fn add_markers_result_stays_small_with_the_longest_names_allowed() {
    let mut f = Fixture::with(vec![]);
    let list: Vec<Value> = (0..40)
        .map(|i| json!({ "name": format!("{i} {}", "y".repeat(190)), "time": i as f64 }))
        .collect();
    let v = f.ok("add_markers", json!({ "markers": list }));
    let size = v.to_string().len();
    assert!(size < 2_000, "the answer is {size} B");
}

// ---------------------------------------------------------------------
// add_markers
// ---------------------------------------------------------------------

#[test]
fn add_markers_places_a_list_of_points_and_regions_in_one_node() {
    let mut f = Fixture::with(vec![marker("existing", 0.5)]);
    let v = f.one_node(|f| {
        f.ok(
            "add_markers",
            json!({ "markers": [
                { "name": "Intro", "time": 0.0 },
                { "name": "Verse", "start": 10.0, "end": 40.0 },
                { "name": "Outro", "time": 90.0 },
            ]}),
        )
    });
    assert_eq!(v["added"], 3);
    assert_eq!(f.annotations().len(), 4);
    assert_eq!(
        f.by_name("Intro").kind,
        AnnotationKind::Marker { time_sec: 0.0 }
    );
    assert_eq!(
        f.by_name("Verse").kind,
        AnnotationKind::Region {
            start_sec: 10.0,
            end_sec: 40.0
        }
    );
    // Each gets an id of its own.
    let mut ids: Vec<_> = f.annotations().into_iter().map(|a| a.id.0).collect();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), 4);
}

#[test]
fn add_markers_agrees_with_the_windows_add() {
    // The window adds `Marker { time_sec }` with the name as typed.
    let mut f = Fixture::with(vec![]);
    f.ok(
        "add_markers",
        json!({ "markers": [{ "name": "Drop", "time": 16.0 }] }),
    );
    assert_eq!(
        f.by_name("Drop").kind,
        AnnotationKind::Marker { time_sec: 16.0 }
    );
}

#[test]
fn add_markers_refuses_a_bad_list_and_adds_none() {
    let mut f = Fixture::with(vec![]);
    let cases = [
        (
            "negative time",
            json!({ "markers": [{ "name": "a", "time": 1.0 }, { "name": "b", "time": -1.0 }] }),
        ),
        (
            "backwards region",
            json!({ "markers": [{ "name": "a", "start": 5.0, "end": 5.0 }] }),
        ),
        ("no time at all", json!({ "markers": [{ "name": "a" }] })),
        (
            "time and region",
            json!({ "markers": [{ "name": "a", "time": 1.0, "start": 1.0, "end": 2.0 }] }),
        ),
        (
            "a lone start",
            json!({ "markers": [{ "name": "a", "start": 1.0 }] }),
        ),
        ("empty list", json!({ "markers": [] })),
        ("neither form", json!({})),
        (
            "both forms",
            json!({ "markers": [{ "name": "a", "time": 1.0 }], "every": 10.0, "to": 60.0 }),
        ),
        (
            "a name too long",
            json!({ "markers": [{ "name": "x".repeat(5_000), "time": 1.0 }] }),
        ),
    ];
    for (why, args) in cases {
        let before = f.head();
        let e = f.refused("add_markers", args);
        assert_eq!(f.head(), before, "{why}: a node was appended ({e})");
        assert!(f.annotations().is_empty(), "{why}");
    }
}

#[test]
fn add_markers_list_is_bounded() {
    let mut f = Fixture::with(vec![]);
    let list = |n: usize| -> Value {
        json!({ "markers": (0..n).map(|i| json!({ "name": format!("m{i}"), "time": i as f64 })).collect::<Vec<_>>() })
    };
    f.ok("add_markers", list(500));
    assert_eq!(f.annotations().len(), 500);
    let e = f.no_node(|f| f.refused("add_markers", list(501)));
    assert!(e.contains("500"), "{e}");
}

#[test]
fn add_markers_every_interval_places_from_plus_i_times_every() {
    let mut f = Fixture::with(vec![]);
    let v = f.one_node(|f| {
        f.ok(
            "add_markers",
            json!({ "every": 15.0, "from": 0.0, "to": 60.0, "name": "Chapter {n}" }),
        )
    });
    assert_eq!(v["added"], 5);
    for (i, t) in [0.0, 15.0, 30.0, 45.0, 60.0].into_iter().enumerate() {
        assert_eq!(
            f.by_name(&format!("Chapter {}", i + 1)).kind,
            AnnotationKind::Marker { time_sec: t }
        );
    }
}

#[test]
fn add_markers_every_interval_does_not_drift() {
    let mut f = Fixture::with(vec![]);
    // 0.1 added a hundred times is 9.99999999999998; 100 * 0.1 is 10.0.
    f.ok(
        "add_markers",
        json!({ "every": 0.1, "from": 0.0, "to": 10.0 }),
    );
    let anns = f.annotations();
    assert_eq!(
        anns.len(),
        101,
        "a marker at 0 and one at 10 are both inside 0..10"
    );
    let last = anns
        .iter()
        .map(|a| match a.kind {
            AnnotationKind::Marker { time_sec } => time_sec,
            _ => unreachable!(),
        })
        .fold(0.0, f64::max);
    assert_eq!(last, 100.0 * 0.1);
}

#[test]
fn add_markers_every_defaults_to_the_session_length_and_a_plain_name() {
    // 1,000 Hz, 30,000 frames: 30 s.
    let mut f = Fixture::with_length(vec![], 30_000);
    f.ok("add_markers", json!({ "every": 10.0, "from": 10.0 }));
    let mut times: Vec<f64> = f
        .annotations()
        .iter()
        .map(|a| match a.kind {
            AnnotationKind::Marker { time_sec } => time_sec,
            _ => unreachable!(),
        })
        .collect();
    times.sort_by(f64::total_cmp);
    assert_eq!(times, [10.0, 20.0, 30.0]);
    assert!(f
        .annotations()
        .iter()
        .all(|a| a.name.starts_with("Marker ")));
}

#[test]
fn add_markers_every_with_no_end_to_go_to_asks_for_one() {
    let mut f = Fixture::with_length(vec![], 0);
    let e = f.no_node(|f| f.refused("add_markers", json!({ "every": 10.0 })));
    assert!(e.contains("to"), "{e}");
}

#[test]
fn add_markers_every_refuses_absurd_counts_and_intervals() {
    let mut f = Fixture::with(vec![]);
    // A marker every millisecond for an hour is 3.6 million.
    let e = f.no_node(|f| f.refused("add_markers", json!({ "every": 0.001, "to": 3600.0 })));
    assert!(e.contains("500"), "the refusal states the bound: {e}");
    assert!(e.contains("every"), "{e}");
    // 501 would be one over.
    f.no_node(|f| {
        f.refused(
            "add_markers",
            json!({ "every": 1.0, "from": 0.0, "to": 500.0 }),
        )
    });
    // 500 exactly is fine: 0..=499.
    f.ok(
        "add_markers",
        json!({ "every": 1.0, "from": 0.0, "to": 499.0 }),
    );
    assert_eq!(f.annotations().len(), 500);

    let mut g = Fixture::with(vec![]);
    for bad in [
        json!({ "every": 0.0, "to": 10.0 }),
        json!({ "every": -5.0, "to": 10.0 }),
        json!({ "every": 5.0, "from": -1.0, "to": 10.0 }),
        json!({ "every": 5.0, "from": 20.0, "to": 10.0 }),
    ] {
        g.no_node(|g| g.refused("add_markers", bad));
    }
}

// ---------------------------------------------------------------------
// Review fixes on #891
// ---------------------------------------------------------------------

/// Fix 4: a region moved to a time so large that its length is lost in
/// floating point would be stored with its end at its start.
#[test]
fn edit_markers_refuses_a_move_that_leaves_a_zero_length_region() {
    let mut f = Fixture::with(vec![region("r", 1.0, 3.0), marker("m", 2.0)]);
    let r = f.id_of("r");
    let before = f.annotations();
    let e = f.no_node(|f| {
        f.refused(
            "edit_markers",
            json!({ "edits": [{ "id": r, "time": 1e17 }] }),
        )
    });
    assert!(e.contains("end > start"), "{e}");
    assert_eq!(f.annotations(), before);
    // Past the largest float is no place either.
    f.no_node(|f| {
        f.refused(
            "edit_markers",
            json!({ "edits": [{ "id": r, "time": 1.7e308 }] }),
        )
    });
    assert_eq!(f.annotations(), before);
    // A marker at a huge time is still a marker.
    let m = f.id_of("m");
    f.ok(
        "edit_markers",
        json!({ "edits": [{ "id": m, "time": 1e17 }] }),
    );
}

/// Fix 5: the refusal quotes what was wrong, and what was wrong may be a
/// 20 KB name or id.
#[test]
fn refusals_do_not_echo_oversized_input() {
    let huge = "z".repeat(20_000);
    let mut f = Fixture::with(vec![marker("m", 1.0)]);
    let m = f.id_of("m");
    for (why, tool, args) in [
        (
            "a long name with a bad time",
            "add_markers",
            json!({ "markers": [{ "name": huge, "time": -1.0 }] }),
        ),
        (
            "a long name that is too long",
            "add_markers",
            json!({ "markers": [{ "name": huge, "time": 1.0 }] }),
        ),
        (
            "a long id that is not an id",
            "edit_markers",
            json!({ "edits": [{ "id": huge, "name": "x" }] }),
        ),
        (
            "a long id twice",
            "edit_markers",
            json!({ "edits": [{ "id": m, "name": "a" }, { "id": m, "name": "b" }, { "id": huge }] }),
        ),
        (
            "a long id to delete",
            "delete_markers",
            json!({ "ids": [huge] }),
        ),
        (
            "a template whose name is too long",
            "add_markers",
            json!({ "every": 1.0, "to": 3.0, "name": huge }),
        ),
    ] {
        let e = f.no_node(|f| f.refused(tool, args));
        assert!(e.len() < 600, "{why}: the refusal is {} B", e.len());
    }
    // A well-formed id that is simply absent is quoted whole: it is short.
    let missing = AnnotationId::new().0.to_string();
    let e = f.refused("delete_markers", json!({ "ids": [missing] }));
    assert!(e.contains(&missing), "{e}");
}

/// Fix 6: `from`, `to` and `name` belong to the interval form. Beside a
/// list they were read and dropped.
#[test]
fn add_markers_refuses_interval_keys_beside_a_list() {
    let mut f = Fixture::with(vec![]);
    for (why, args) in [
        (
            "from",
            json!({ "markers": [{ "name": "a", "time": 1.0 }], "from": 10.0 }),
        ),
        (
            "to",
            json!({ "markers": [{ "name": "a", "time": 1.0 }], "to": 60.0 }),
        ),
        (
            "name",
            json!({ "markers": [{ "name": "a", "time": 1.0 }], "name": "Chapter {n}" }),
        ),
        ("from alone", json!({ "from": 10.0, "to": 60.0 })),
    ] {
        let e = f.no_node(|f| f.refused("add_markers", args));
        assert!(
            e.contains("every") || e.contains("markers"),
            "{why}: the refusal says which form: {e}"
        );
        assert!(f.annotations().is_empty(), "{why}");
    }
}

/// Fix 8: the end of an interval is a step like any other when it falls on
/// one, and the schema says so.
#[test]
fn add_markers_every_includes_the_end_and_the_schema_says_so() {
    let mut f = Fixture::with(vec![]);
    let v = f.ok(
        "add_markers",
        json!({ "every": 600.0, "from": 0.0, "to": 3_600.0 }),
    );
    assert_eq!(v["added"], 7, "0, 600, ... 3600");
    let schema = ToolDispatcher::default_dispatcher().tool_schemas();
    let text = schema
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "add_markers")
        .expect("add_markers")
        .to_string();
    assert!(text.contains("including `to`"), "{text}");
}
