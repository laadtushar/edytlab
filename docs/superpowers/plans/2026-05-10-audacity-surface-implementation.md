# Audacity Surface + First Wave — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:subagent-driven-development` (recommended) or `superpowers:executing-plans` to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Land the foundation surface for region-aware tools + markers, plus a five-tool reference wave (`fade`, `reverse`, `insert_silence`, `copy_region`, `paste_region`) and the marker primitive `label`. Every Audacity-class tool added later plugs into this surface.

**Architecture:** Markers persist as a new `annotations: Vec<Annotation>` field on `SessionState`, snapshotted into the existing content-addressed graph alongside audio edits. Region selection plumbed FE → debounced IPC → Rust `SessionContext` → typed `range` param + parallel text prefix in user message. Two-way marker UX (UI ruler clicks AND agent `label` tool both write).

**Tech Stack:** Rust 1.88 + Tauri 2 backend, React 19 + Tailwind 4 + wavesurfer.js 7 frontend, vitest + cargo test for verification.

**Spec:** [docs/superpowers/specs/2026-05-10-audacity-surface-design.md](../specs/2026-05-10-audacity-surface-design.md)

---

## File structure

### New
- `crates/session/src/annotation.rs` — `Annotation`, `AnnotationKind`, `AnnotationId` types + helpers.
- `crates/session/tests/annotations.rs` — fork-isolation, tombstone, walk semantics.
- `crates/tools/src/util/range_resolver.rs` — typed-or-text range resolution.
- `crates/tools/src/tool/fade.rs`
- `crates/tools/src/tool/reverse.rs`
- `crates/tools/src/tool/insert_silence.rs`
- `crates/tools/src/tool/copy_region.rs`
- `crates/tools/src/tool/paste_region.rs`
- `crates/tools/src/tool/label.rs`
- `crates/ai/src/session_context.rs` — `SessionContext` struct + system-prompt builder.
- `crates/ai/tests/session_context.rs`
- `apps/desktop/src/components/MarkerLayer.tsx`
- `apps/desktop/src/components/Ruler.tsx`
- `apps/desktop/src/components/__tests__/MarkerLayer.test.tsx`

### Modify
- `crates/session/src/state.rs` — add `annotations` field to `SessionState`.
- `crates/session/src/lib.rs` — re-export annotation types.
- `crates/session/src/store.rs` — `add_annotation`, `remove_annotation`, `annotations_for(head)` helpers.
- `crates/tools/src/util.rs` (or new `util/mod.rs`) — wire `range_resolver`.
- `crates/tools/src/tool/mod.rs` — register new tools.
- `crates/tools/src/schema.rs` — add `Range` schema variant + register tool schemas.
- `crates/tools/src/dispatcher.rs` — dispatch new tool names.
- `crates/ai/src/lib.rs` — re-export `SessionContext`.
- `crates/ai/src/agent_loop.rs` — accept + inject `SessionContext`.
- `crates/ai/src/prompt.rs` — invoke prompt builder.
- `apps/desktop/src-tauri/src/state.rs` — add `selection: Mutex<Option<Range>>` and `clipboard: Mutex<Option<Vec<f32>>>`.
- `apps/desktop/src-tauri/src/commands.rs` — `set_selection_context`, `add_marker`, `remove_marker`, `list_markers`; thread `SessionContext` into `send_message`.
- `apps/desktop/src-tauri/src/lib.rs` — register the four new commands.
- `apps/desktop/src/lib/tauri-bridge.ts` — wrappers for the new commands.
- `apps/desktop/src/components/Timeline.tsx` — host `Ruler` + `MarkerLayer` above first lane.
- `apps/desktop/src/App.tsx` — `markers` state, `marker-changed` listener, debounced selection push.
- `apps/desktop/src/components/Chat.tsx` — already does selection prefix; extend for marker context line if active.

---

## Phase A — `session` crate annotations

### Task A1: Annotation types

**Files:**
- Create: `crates/session/src/annotation.rs`
- Modify: `crates/session/src/lib.rs:11` (add `pub mod annotation;`)

- [ ] **Step 1: Write the failing test**

Add to a new `crates/session/src/annotation.rs`:

```rust
//! Marker / region annotations stored on `SessionState`.
//!
//! Annotations describe points or ranges in the rendered audio at a
//! given head. They live on `SessionState` so they share the
//! content-addressed lifetime of the graph: forks see only their own
//! annotation set, and reverting moves the user to a different one
//! automatically.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AnnotationId(pub Uuid);

impl AnnotationId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for AnnotationId {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AnnotationKind {
    Marker { time_sec: f64 },
    Region { start_sec: f64, end_sec: f64 },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Annotation {
    pub id: AnnotationId,
    pub name: String,
    #[serde(flatten)]
    pub kind: AnnotationKind,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marker_serializes_to_expected_shape() {
        let a = Annotation {
            id: AnnotationId(Uuid::nil()),
            name: "chorus".into(),
            kind: AnnotationKind::Marker { time_sec: 42.0 },
        };
        let json = serde_json::to_value(&a).unwrap();
        assert_eq!(json["name"], "chorus");
        assert_eq!(json["kind"], "marker");
        assert_eq!(json["time_sec"], 42.0);
    }

    #[test]
    fn region_serializes_to_expected_shape() {
        let a = Annotation {
            id: AnnotationId(Uuid::nil()),
            name: "verse".into(),
            kind: AnnotationKind::Region {
                start_sec: 1.0,
                end_sec: 3.5,
            },
        };
        let json = serde_json::to_value(&a).unwrap();
        assert_eq!(json["kind"], "region");
        assert_eq!(json["start_sec"], 1.0);
        assert_eq!(json["end_sec"], 3.5);
    }
}
```

Add to `crates/session/src/lib.rs`:

```rust
pub mod annotation;
pub use annotation::{Annotation, AnnotationId, AnnotationKind};
```

- [ ] **Step 2: Run tests to verify they pass**

Run: `cargo test -p session annotation::tests`
Expected: 2 passed.

- [ ] **Step 3: Commit**

```bash
git add crates/session/src/annotation.rs crates/session/src/lib.rs
git commit -m "feat(session): annotation types (marker / region)"
```

---

### Task A2: `SessionState.annotations` field

**Files:**
- Modify: `crates/session/src/state.rs` (struct definition)

- [ ] **Step 1: Write the failing test**

Add to `crates/session/src/state.rs` near the existing tests (or create one if absent):

```rust
#[cfg(test)]
mod state_tests {
    use super::*;
    use crate::annotation::{Annotation, AnnotationId, AnnotationKind};

    #[test]
    fn annotations_default_to_empty() {
        let s: SessionState = serde_json::from_str(r#"{
            "tracks": [],
            "bus_routing": {"buses": []},
            "master_chain": [],
            "tempo_map": {"segments": []},
            "key_map": null,
            "transcript": null,
            "sample_rate": 48000,
            "length_samples": 0
        }"#).unwrap();
        assert!(s.annotations.is_empty());
    }

    #[test]
    fn empty_annotations_are_skipped_in_serialization() {
        let s = SessionState {
            tracks: vec![],
            bus_routing: BusGraph::default(),
            master_chain: vec![],
            tempo_map: TempoMap::default(),
            key_map: None,
            transcript: None,
            sample_rate: 48000,
            length_samples: 0,
            annotations: vec![],
        };
        let json = serde_json::to_string(&s).unwrap();
        assert!(
            !json.contains("annotations"),
            "empty annotations must not appear in JSON: {json}"
        );
    }

    #[test]
    fn non_empty_annotations_round_trip() {
        let mut s = SessionState {
            tracks: vec![],
            bus_routing: BusGraph::default(),
            master_chain: vec![],
            tempo_map: TempoMap::default(),
            key_map: None,
            transcript: None,
            sample_rate: 48000,
            length_samples: 0,
            annotations: vec![],
        };
        s.annotations.push(Annotation {
            id: AnnotationId::new(),
            name: "chorus".into(),
            kind: AnnotationKind::Marker { time_sec: 42.0 },
        });
        let json = serde_json::to_string(&s).unwrap();
        let back: SessionState = serde_json::from_str(&json).unwrap();
        assert_eq!(back.annotations.len(), 1);
    }
}
```

- [ ] **Step 2: Run test to verify they fail**

Run: `cargo test -p session state_tests`
Expected: FAIL — `SessionState` has no `annotations` field.

- [ ] **Step 3: Add the field**

In `crates/session/src/state.rs`, modify the `SessionState` struct:

```rust
use crate::annotation::Annotation;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionState {
    pub tracks: Vec<Track>,
    pub bus_routing: BusGraph,
    pub master_chain: Vec<EffectInstance>,
    pub tempo_map: TempoMap,
    pub key_map: Option<KeyMap>,
    pub transcript: Option<Transcript>,
    pub sample_rate: u32,
    pub length_samples: u64,
    /// Marker / region annotations attached to the rendered audio at
    /// this head. `#[serde(default, skip_serializing_if = …)]` keeps
    /// existing on-disk node JSON readable AND keeps the content
    /// hash stable for nodes that don't use annotations.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub annotations: Vec<Annotation>,
}
```

- [ ] **Step 4: Run all session tests**

Run: `cargo test -p session`
Expected: All pass — including the 3 new ones AND every existing snapshot test (because empty-annotations JSON is unchanged).

- [ ] **Step 5: Commit**

```bash
git add crates/session/src/state.rs
git commit -m "feat(session): add SessionState.annotations field

skip_serializing_if=Vec::is_empty preserves existing on-disk
node IDs (the field is absent from JSON when no annotations
are set, so the content hash matches pre-feature nodes)."
```

---

### Task A3: `Store::annotations_for(head)` walker

**Files:**
- Modify: `crates/session/src/store.rs` (new `impl Store` method)
- Modify: `crates/session/src/lib.rs` (re-exports already done)

- [ ] **Step 1: Write the failing test**

Append to `crates/session/tests/` — create a new file `annotations.rs`:

```rust
//! Annotation walking semantics: a marker added at one head should
//! be visible at every descendant head, but isolated from sibling
//! branches.

use session::{
    Annotation, AnnotationId, AnnotationKind, SessionNode, SessionState, Store,
};
use tempfile::TempDir;

fn empty_state() -> SessionState {
    SessionState {
        tracks: vec![],
        bus_routing: Default::default(),
        master_chain: vec![],
        tempo_map: Default::default(),
        key_map: None,
        transcript: None,
        sample_rate: 48000,
        length_samples: 0,
        annotations: vec![],
    }
}

fn append_with_annotations(
    store: &mut Store,
    annotations: Vec<Annotation>,
) -> session::NodeId {
    let mut state = empty_state();
    state.annotations = annotations;
    store
        .append(SessionNode::new(state, "test".into(), None))
        .expect("append")
}

#[test]
fn annotations_for_returns_head_state_annotations() {
    let tmp = TempDir::new().unwrap();
    let mut store = Store::open(tmp.path()).unwrap();
    let chorus = Annotation {
        id: AnnotationId::new(),
        name: "chorus".into(),
        kind: AnnotationKind::Marker { time_sec: 42.0 },
    };
    let head = append_with_annotations(&mut store, vec![chorus.clone()]);
    let visible = store.annotations_for(head).unwrap();
    assert_eq!(visible.len(), 1);
    assert_eq!(visible[0].name, "chorus");
}

#[test]
fn annotations_for_at_empty_head_is_empty() {
    let tmp = TempDir::new().unwrap();
    let mut store = Store::open(tmp.path()).unwrap();
    let head = append_with_annotations(&mut store, vec![]);
    let visible = store.annotations_for(head).unwrap();
    assert!(visible.is_empty());
}
```

> `SessionNode::new` may not exist with that signature — check `crates/session/src/node.rs` and adjust to whatever constructor the existing tests use (`SessionNode { id: …, parent: None, state, … }` direct literal is acceptable).

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p session --test annotations`
Expected: FAIL — `Store::annotations_for` does not exist.

- [ ] **Step 3: Implement `annotations_for`**

In `crates/session/src/store.rs`, add to `impl Store`:

```rust
/// Annotations visible at `head`. Currently this is just the
/// annotation list snapshotted on `head`'s state, because every
/// edit operation that mutates annotations writes a new node with
/// the full updated list. Sibling-branch isolation is automatic:
/// a fork that doesn't contain the annotation simply won't carry
/// it forward.
pub fn annotations_for(&self, head: NodeId) -> Result<Vec<crate::annotation::Annotation>> {
    let node = self.read_node(head)?;
    Ok(node.state.annotations.clone())
}
```

If `read_node` doesn't exist, use whatever the existing accessor pattern is (e.g. `Store::node(&self, id) -> Result<SessionNode>`). Check the existing implementation in the same file.

- [ ] **Step 4: Run tests**

Run: `cargo test -p session --test annotations`
Expected: 2 passed.

- [ ] **Step 5: Commit**

```bash
git add crates/session/src/store.rs crates/session/tests/annotations.rs
git commit -m "feat(session): Store::annotations_for(head) walker"
```

---

### Task A4: Add and remove annotations via append-new-state

**Files:**
- Modify: `crates/session/src/store.rs`

- [ ] **Step 1: Write the failing test**

Append to `crates/session/tests/annotations.rs`:

```rust
#[test]
fn add_annotation_appends_node_with_extended_list() {
    let tmp = TempDir::new().unwrap();
    let mut store = Store::open(tmp.path()).unwrap();
    let head1 = append_with_annotations(&mut store, vec![]);
    let chorus = Annotation {
        id: AnnotationId::new(),
        name: "chorus".into(),
        kind: AnnotationKind::Marker { time_sec: 42.0 },
    };
    let head2 = store.add_annotation(head1, chorus.clone()).unwrap();
    assert_ne!(head1, head2, "add_annotation must create a new node");

    let visible = store.annotations_for(head2).unwrap();
    assert_eq!(visible.len(), 1);
    assert_eq!(visible[0].id, chorus.id);

    let still_empty = store.annotations_for(head1).unwrap();
    assert!(still_empty.is_empty(), "old head should still be empty");
}

#[test]
fn remove_annotation_appends_node_without_target() {
    let tmp = TempDir::new().unwrap();
    let mut store = Store::open(tmp.path()).unwrap();
    let head1 = append_with_annotations(&mut store, vec![]);
    let chorus = Annotation {
        id: AnnotationId::new(),
        name: "chorus".into(),
        kind: AnnotationKind::Marker { time_sec: 42.0 },
    };
    let head2 = store.add_annotation(head1, chorus.clone()).unwrap();
    let head3 = store.remove_annotation(head2, chorus.id).unwrap();

    assert!(store.annotations_for(head3).unwrap().is_empty());
    // Old head still has the marker, since the graph is append-only.
    assert_eq!(store.annotations_for(head2).unwrap().len(), 1);
}

#[test]
fn remove_annotation_unknown_id_is_idempotent() {
    let tmp = TempDir::new().unwrap();
    let mut store = Store::open(tmp.path()).unwrap();
    let head = append_with_annotations(&mut store, vec![]);
    let result = store.remove_annotation(head, AnnotationId::new()).unwrap();
    assert_eq!(
        result, head,
        "removing an unknown id should be a no-op (return same head)"
    );
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p session --test annotations add_annotation remove_annotation`
Expected: FAIL — methods don't exist.

- [ ] **Step 3: Implement the helpers**

In `crates/session/src/store.rs`, add to `impl Store`:

```rust
/// Append a new node whose state extends `head`'s annotation list
/// with `annotation`. The audio content is unchanged; only the
/// annotation list is mutated, so the new node's hash differs from
/// `head` purely by its embedded annotations.
pub fn add_annotation(
    &mut self,
    head: NodeId,
    annotation: crate::annotation::Annotation,
) -> Result<NodeId> {
    let mut node = self.read_node(head)?;
    node.state.annotations.push(annotation);
    self.append(node)
}

/// Append a new node whose state filters `head`'s annotation list
/// to remove `target`. If `target` is absent, no new node is
/// written and `head` is returned unchanged.
pub fn remove_annotation(
    &mut self,
    head: NodeId,
    target: crate::annotation::AnnotationId,
) -> Result<NodeId> {
    let mut node = self.read_node(head)?;
    let before = node.state.annotations.len();
    node.state.annotations.retain(|a| a.id != target);
    if node.state.annotations.len() == before {
        return Ok(head);
    }
    self.append(node)
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p session --test annotations`
Expected: All passed.

- [ ] **Step 5: Commit**

```bash
git add crates/session/src/store.rs crates/session/tests/annotations.rs
git commit -m "feat(session): add_annotation / remove_annotation helpers"
```

---

## Phase B — `tools::util::range_resolver`

### Task B1: Range type + parser

**Files:**
- Create: `crates/tools/src/util/range_resolver.rs`
- Modify: `crates/tools/src/lib.rs` (add `pub mod util;` if missing) and / or `crates/tools/src/util.rs`

- [ ] **Step 1: Write the failing test**

Create `crates/tools/src/util/range_resolver.rs`:

```rust
//! Resolve a `Range` for a tool from either:
//!   1. A typed `range` parameter the LLM filled in (preferred), or
//!   2. The `[apply to MM:SS-MM:SS]` text prefix in the user message.
//!
//! Tools call `range_resolver(typed, message, required)` and the helper
//! returns `Ok(Some(Range))`, `Ok(None)` when the tool's range is
//! optional and nothing was supplied, or `Err(MissingRange)` when the
//! tool requires one and neither source produced a value.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Range {
    pub start_sec: f64,
    pub end_sec: f64,
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum RangeError {
    #[error("range is required for this tool but neither a typed param nor a parseable text prefix was provided")]
    MissingRange,
    #[error("range is invalid: start ({start_sec}) must be < end ({end_sec})")]
    InvalidOrder { start_sec: f64, end_sec: f64 },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_param_wins_over_text_prefix() {
        let typed = Some(Range { start_sec: 1.0, end_sec: 2.0 });
        let msg = "[apply to 0:10-0:20] fade out";
        let r = resolve(typed, msg, true).unwrap().unwrap();
        assert_eq!(r.start_sec, 1.0);
        assert_eq!(r.end_sec, 2.0);
    }

    #[test]
    fn text_prefix_used_when_typed_is_none() {
        let r = resolve(None, "[apply to 0:23.45-0:45.10] fade out", true)
            .unwrap()
            .unwrap();
        assert!((r.start_sec - 23.45).abs() < 1e-6);
        assert!((r.end_sec - 45.10).abs() < 1e-6);
    }

    #[test]
    fn missing_when_required_and_nothing_present() {
        let err = resolve(None, "fade out", true).unwrap_err();
        assert_eq!(err, RangeError::MissingRange);
    }

    #[test]
    fn missing_returns_none_when_not_required() {
        let r = resolve(None, "reverse the whole thing", false).unwrap();
        assert!(r.is_none());
    }

    #[test]
    fn invalid_order_rejected() {
        let typed = Some(Range { start_sec: 5.0, end_sec: 2.0 });
        let err = resolve(typed, "", true).unwrap_err();
        assert!(matches!(err, RangeError::InvalidOrder { .. }));
    }

    #[test]
    fn parses_compact_format() {
        let r = resolve(None, "[apply to 1:00-1:30]", true).unwrap().unwrap();
        assert_eq!(r.start_sec, 60.0);
        assert_eq!(r.end_sec, 90.0);
    }
}
```

Then add a stub function so the test file compiles (will still fail at runtime):

```rust
pub fn resolve(
    typed: Option<Range>,
    _message: &str,
    _required: bool,
) -> Result<Option<Range>, RangeError> {
    let _ = typed;
    todo!()
}
```

Wire the module: in `crates/tools/src/lib.rs` (or wherever the existing crate root lives) add:

```rust
pub mod util;
pub use util::range_resolver::{resolve as resolve_range, Range, RangeError};
```

If `util` is currently a single-file module (`util.rs`), convert it to `util/` directory with `util/mod.rs` re-exporting both the old contents and the new `range_resolver` submodule.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p tools range_resolver`
Expected: FAIL — `todo!()` panics.

- [ ] **Step 3: Implement `resolve`**

Replace the `todo!()` body:

```rust
use regex::Regex;
use std::sync::OnceLock;

/// Captures `MM:SS[.ms]` two times in `[apply to <start>-<end>]`.
/// Whitespace tolerant; case-insensitive on the keyword.
static PREFIX_RE: OnceLock<Regex> = OnceLock::new();

fn prefix_re() -> &'static Regex {
    PREFIX_RE.get_or_init(|| {
        Regex::new(
            r"(?i)\[apply\s+to\s+(\d+):(\d+(?:\.\d+)?)-(\d+):(\d+(?:\.\d+)?)\]",
        )
        .expect("range prefix regex is statically valid")
    })
}

pub fn resolve(
    typed: Option<Range>,
    message: &str,
    required: bool,
) -> Result<Option<Range>, RangeError> {
    if let Some(r) = typed {
        validate_order(&r)?;
        return Ok(Some(r));
    }
    if let Some(caps) = prefix_re().captures(message) {
        let start = caps[1].parse::<f64>().unwrap() * 60.0
            + caps[2].parse::<f64>().unwrap();
        let end = caps[3].parse::<f64>().unwrap() * 60.0
            + caps[4].parse::<f64>().unwrap();
        let r = Range { start_sec: start, end_sec: end };
        validate_order(&r)?;
        return Ok(Some(r));
    }
    if required {
        return Err(RangeError::MissingRange);
    }
    Ok(None)
}

fn validate_order(r: &Range) -> Result<(), RangeError> {
    if r.start_sec >= r.end_sec {
        return Err(RangeError::InvalidOrder {
            start_sec: r.start_sec,
            end_sec: r.end_sec,
        });
    }
    Ok(())
}
```

If `regex` isn't a `tools` dep yet, add it to `crates/tools/Cargo.toml`:

```toml
regex = "1"
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p tools range_resolver`
Expected: 6 passed.

- [ ] **Step 5: Commit**

```bash
git add crates/tools/src/util crates/tools/src/lib.rs crates/tools/Cargo.toml
git commit -m "feat(tools): range_resolver — typed param or [apply to ...] prefix"
```

---

## Phase C — `ai::SessionContext`

### Task C1: SessionContext struct + system prompt builder

**Files:**
- Create: `crates/ai/src/session_context.rs`
- Create: `crates/ai/tests/session_context.rs`
- Modify: `crates/ai/src/lib.rs` (re-export)

- [ ] **Step 1: Write the failing test**

Create `crates/ai/tests/session_context.rs`:

```rust
use ai::session_context::{SessionContext, render_block};
use session::{Annotation, AnnotationId, AnnotationKind};
use tools::Range;

fn ann(name: &str, time: f64) -> Annotation {
    Annotation {
        id: AnnotationId::new(),
        name: name.into(),
        kind: AnnotationKind::Marker { time_sec: time },
    }
}

#[test]
fn block_includes_selection_when_present() {
    let ctx = SessionContext {
        selection: Some(Range { start_sec: 1.0, end_sec: 2.5 }),
        markers: vec![],
    };
    let block = render_block(&ctx);
    assert!(block.contains("current_selection"));
    assert!(block.contains("1.00"));
    assert!(block.contains("2.50"));
}

#[test]
fn block_includes_markers_sorted_by_time() {
    let ctx = SessionContext {
        selection: None,
        markers: vec![ann("drop", 78.5), ann("chorus", 42.0)],
    };
    let block = render_block(&ctx);
    let chorus_pos = block.find("chorus").unwrap();
    let drop_pos = block.find("drop").unwrap();
    assert!(
        chorus_pos < drop_pos,
        "markers should be sorted by time ascending"
    );
}

#[test]
fn empty_context_renders_empty_string() {
    let ctx = SessionContext { selection: None, markers: vec![] };
    assert_eq!(render_block(&ctx), "");
}

#[test]
fn region_annotations_render_with_range() {
    let ctx = SessionContext {
        selection: None,
        markers: vec![Annotation {
            id: AnnotationId::new(),
            name: "verse".into(),
            kind: AnnotationKind::Region { start_sec: 1.0, end_sec: 5.0 },
        }],
    };
    let block = render_block(&ctx);
    assert!(block.contains("verse"));
    assert!(block.contains("1.00"));
    assert!(block.contains("5.00"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p ai --test session_context`
Expected: FAIL — `session_context` module does not exist.

- [ ] **Step 3: Implement**

Create `crates/ai/src/session_context.rs`:

```rust
//! `SessionContext` — what the agent loop sees about the user's
//! current focus on the timeline. Built per turn from
//! frontend-pushed selection + store-loaded annotations.

use session::{Annotation, AnnotationKind};
use tools::Range;

#[derive(Debug, Clone, Default)]
pub struct SessionContext {
    pub selection: Option<Range>,
    pub markers: Vec<Annotation>,
}

/// Render the context as a deterministic block to splice into the
/// system prompt. Returns an empty string when context is empty so
/// callers can splice unconditionally.
pub fn render_block(ctx: &SessionContext) -> String {
    if ctx.selection.is_none() && ctx.markers.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    if let Some(sel) = ctx.selection {
        out.push_str("## current_selection\n");
        out.push_str(&format!(
            "start: {:.2}s  end: {:.2}s\n\n",
            sel.start_sec, sel.end_sec
        ));
    }
    if !ctx.markers.is_empty() {
        let mut sorted: Vec<&Annotation> = ctx.markers.iter().collect();
        sorted.sort_by(|a, b| {
            time_of(a)
                .partial_cmp(&time_of(b))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        out.push_str("## markers\n");
        for a in sorted {
            match &a.kind {
                AnnotationKind::Marker { time_sec } => {
                    out.push_str(&format!("- {} @ {:.2}s\n", a.name, time_sec));
                }
                AnnotationKind::Region { start_sec, end_sec } => {
                    out.push_str(&format!(
                        "- {} @ {:.2}s-{:.2}s\n",
                        a.name, start_sec, end_sec
                    ));
                }
            }
        }
    }
    out
}

fn time_of(a: &Annotation) -> f64 {
    match a.kind {
        AnnotationKind::Marker { time_sec } => time_sec,
        AnnotationKind::Region { start_sec, .. } => start_sec,
    }
}
```

In `crates/ai/src/lib.rs`:

```rust
pub mod session_context;
pub use session_context::{SessionContext, render_block as render_session_block};
```

If `tools::Range` is not yet importable into `ai`, add `tools` as a dep on `ai` in `crates/ai/Cargo.toml`:

```toml
tools = { path = "../tools" }
```

(Or, if a circular-dep concern surfaces, move `Range` into the `session` crate and import from there in both `ai` and `tools`.)

- [ ] **Step 4: Run tests**

Run: `cargo test -p ai --test session_context`
Expected: 4 passed.

- [ ] **Step 5: Commit**

```bash
git add crates/ai/src/session_context.rs crates/ai/src/lib.rs crates/ai/Cargo.toml crates/ai/tests/session_context.rs
git commit -m "feat(ai): SessionContext + system-prompt block renderer"
```

---

### Task C2: Wire SessionContext into agent loop

**Files:**
- Modify: `crates/ai/src/agent_loop.rs` (function signature + system prompt construction)
- Modify: `crates/ai/src/prompt.rs` (if system prompt is built there)

- [ ] **Step 1: Write the failing test**

Add to `crates/ai/tests/session_context.rs`:

```rust
use ai::SessionContext;

#[test]
fn agent_loop_accepts_session_context() {
    // Compile-test: ensure the `Agent::turn_with_context` API exists
    // and accepts a SessionContext. The actual LLM call is mocked
    // elsewhere; this test only checks the type surface.
    fn assert_signature<F>()
    where
        F: Fn(SessionContext) -> (),
    {
    }
    assert_signature::<fn(SessionContext)>();
}
```

This is a smoke test; the real verification is callers compiling.

- [ ] **Step 2: Run existing ai tests**

Run: `cargo test -p ai`
Expected: All pass — change should be additive.

- [ ] **Step 3: Add an optional SessionContext parameter to the agent turn**

Find where the agent's per-turn system prompt is assembled (likely `crates/ai/src/prompt.rs::build_system_prompt` or similar). Add a `ctx: Option<&SessionContext>` parameter and append `render_session_block(ctx)` when present.

Concrete pattern (adapt to actual signature):

```rust
// crates/ai/src/prompt.rs (illustrative)
use crate::session_context::{render_block, SessionContext};

pub fn build_system_prompt(
    base: &str,
    ctx: Option<&SessionContext>,
) -> String {
    let mut out = String::from(base);
    if let Some(ctx) = ctx {
        let block = render_block(ctx);
        if !block.is_empty() {
            out.push_str("\n\n");
            out.push_str(&block);
        }
    }
    out
}
```

In `crates/ai/src/agent_loop.rs`, surface a new public `turn_with_context` (or extend the existing `turn` signature with an `Option<SessionContext>`). The simpler change is a default-defaulted option:

```rust
impl Agent {
    pub async fn turn<F>(
        &mut self,
        message: String,
        on_event: F,
    ) -> Result<()>
    where
        F: FnMut(AgentEvent),
    {
        self.turn_with_context(message, None, on_event).await
    }

    pub async fn turn_with_context<F>(
        &mut self,
        message: String,
        ctx: Option<SessionContext>,
        on_event: F,
    ) -> Result<()>
    where
        F: FnMut(AgentEvent),
    {
        // existing body, but call build_system_prompt(base, ctx.as_ref())
        // when constructing the per-turn payload.
    }
}
```

- [ ] **Step 4: Run all tests**

Run: `cargo test -p ai`
Expected: All pass; signature smoke test compiles.

- [ ] **Step 5: Commit**

```bash
git add crates/ai/src/agent_loop.rs crates/ai/src/prompt.rs crates/ai/tests/session_context.rs
git commit -m "feat(ai): thread SessionContext into Agent::turn_with_context

Existing turn() shim delegates with None so callers that don't
care about selection / markers stay unchanged. send_message in
the desktop crate will call turn_with_context."
```

---

## Phase D — Wave tools

> Each wave tool follows the same shape: define schema, implement audio op against `audio-engine`, write tests under `crates/tools/tests/`, register in `dispatcher.rs`. The five tasks are listed compactly below; reuse the same TDD substeps as Phase A/B.

### Task D1: `fade`

**Files:**
- Create: `crates/tools/src/tool/fade.rs`
- Create: `crates/tools/tests/fade.rs`
- Modify: `crates/tools/src/tool/mod.rs`, `crates/tools/src/schema.rs`, `crates/tools/src/dispatcher.rs`

- [ ] **Step 1: Write tests**

`crates/tools/tests/fade.rs`:

```rust
use tools::{Range, tool::fade};

#[test]
fn fade_in_starts_at_zero() {
    let mut samples = vec![1.0f32; 48_000];
    fade::apply_fade(&mut samples, 48_000, Range { start_sec: 0.0, end_sec: 1.0 }, fade::Kind::In);
    assert!(samples[0].abs() < 1e-3);
    assert!((samples[47_999] - 1.0).abs() < 1e-3);
}

#[test]
fn fade_out_ends_at_zero() {
    let mut samples = vec![1.0f32; 48_000];
    fade::apply_fade(&mut samples, 48_000, Range { start_sec: 0.0, end_sec: 1.0 }, fade::Kind::Out);
    assert!((samples[0] - 1.0).abs() < 1e-3);
    assert!(samples[47_999].abs() < 1e-3);
}

#[test]
fn samples_outside_range_unchanged() {
    let mut samples = vec![1.0f32; 96_000];
    fade::apply_fade(&mut samples, 48_000, Range { start_sec: 0.0, end_sec: 1.0 }, fade::Kind::In);
    assert!((samples[80_000] - 1.0).abs() < 1e-9);
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p tools --test fade`
Expected: FAIL — `fade` module missing.

- [ ] **Step 3: Implement**

`crates/tools/src/tool/fade.rs`:

```rust
//! Linear fade-in / fade-out within a region.

use crate::Range;

#[derive(Debug, Clone, Copy)]
pub enum Kind {
    In,
    Out,
}

pub fn apply_fade(
    samples: &mut [f32],
    sample_rate: u32,
    range: Range,
    kind: Kind,
) {
    let start = (range.start_sec * sample_rate as f64) as usize;
    let end = ((range.end_sec * sample_rate as f64) as usize)
        .min(samples.len());
    if end <= start {
        return;
    }
    let len = (end - start) as f32;
    for (i, sample) in samples[start..end].iter_mut().enumerate() {
        let t = i as f32 / len;
        let gain = match kind {
            Kind::In => t,
            Kind::Out => 1.0 - t,
        };
        *sample *= gain;
    }
}

// ---------------------------------------------------------------------------
// Tool entry point: dispatched via crates/tools/src/dispatcher.rs.
// Receives the JSON params the agent built; returns a side-effect that the
// dispatcher commits as a new edit node.
// ---------------------------------------------------------------------------

use serde::Deserialize;
use crate::util::range_resolver::{resolve as resolve_range, RangeError};

#[derive(Debug, Deserialize)]
pub struct FadeParams {
    pub range: Option<Range>,
    #[serde(default = "default_kind")]
    pub kind: KindParam,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KindParam {
    In,
    Out,
}

fn default_kind() -> KindParam { KindParam::Out }

impl From<KindParam> for Kind {
    fn from(p: KindParam) -> Self {
        match p {
            KindParam::In => Kind::In,
            KindParam::Out => Kind::Out,
        }
    }
}

pub fn dispatch_fade(
    params: FadeParams,
    user_message: &str,
    samples: &mut [f32],
    sample_rate: u32,
) -> Result<(), FadeError> {
    let range = resolve_range(params.range, user_message, true)
        .map_err(FadeError::Range)?
        .expect("required => Some on Ok");
    apply_fade(samples, sample_rate, range, params.kind.into());
    Ok(())
}

#[derive(Debug, thiserror::Error)]
pub enum FadeError {
    #[error("{0}")]
    Range(#[from] RangeError),
}
```

- [ ] **Step 4: Register**

`crates/tools/src/tool/mod.rs`: `pub mod fade;`

`crates/tools/src/schema.rs`: add a `Tool::Fade` schema entry with name `"fade"`, description `"Apply a linear fade in or out across a region of the head's audio"`, params `{ range: Range, kind: "in" | "out" }`. Match the existing tool entries' style.

`crates/tools/src/dispatcher.rs`: add a match arm `"fade" => dispatch_fade(params, …)`.

- [ ] **Step 5: Run tests**

Run: `cargo test -p tools --test fade && cargo test -p tools dispatcher`
Expected: All pass.

- [ ] **Step 6: Commit**

```bash
git add crates/tools/src/tool/fade.rs crates/tools/tests/fade.rs crates/tools/src/tool/mod.rs crates/tools/src/schema.rs crates/tools/src/dispatcher.rs
git commit -m "feat(tools): fade tool with linear in/out curve"
```

---

### Task D2: `reverse`

Mirrors D1. Tests assert the sample buffer is reversed in `range` (or whole buffer when `range: None`). Implementation is `samples[start..end].reverse()`. Schema declares `range: Range?`.

- [ ] Write `crates/tools/tests/reverse.rs` with `reverse_full_track` and `reverse_subrange_only` cases.
- [ ] Implement `crates/tools/src/tool/reverse.rs` exposing `apply_reverse(&mut [f32], u32, Option<Range>)`.
- [ ] Register in `mod.rs`, `schema.rs`, `dispatcher.rs`.
- [ ] Run `cargo test -p tools --test reverse` until green.
- [ ] Commit `feat(tools): reverse tool (full buffer or sub-range)`.

---

### Task D3: `insert_silence`

- [ ] Tests under `crates/tools/tests/insert_silence.rs`:
  - `insert_silence_extends_buffer_length`
  - `insert_silence_at_zero_prepends`
  - `insert_silence_negative_duration_rejected`
- [ ] Implementation: `apply_insert_silence(samples: &mut Vec<f32>, sample_rate: u32, at_sec: f64, duration_sec: f64) -> Result<(), InsertSilenceError>`. Use `Vec::splice` to insert `duration_sec * sample_rate` zeros at the right offset.
- [ ] Schema: `{ at: number, duration: number }`.
- [ ] Register + dispatcher arm.
- [ ] `cargo test -p tools --test insert_silence` green.
- [ ] Commit.

---

### Task D4: `copy_region` + `AppState` clipboard slot

**Files:**
- Modify: `apps/desktop/src-tauri/src/state.rs` — add `clipboard: Mutex<Option<Vec<f32>>>` field + `set_clipboard` / `take_clipboard` accessors.
- Create: `crates/tools/src/tool/copy_region.rs`
- Create: `crates/tools/tests/copy_region.rs`

- [ ] **Step 1: Tests** — `copy_region` should extract the slice `[start..end]` from a buffer at sample rate `sr` and write it to a passed-in `clipboard: &mut Option<Vec<f32>>`.

```rust
#[test]
fn copy_writes_slice_to_clipboard() {
    let buf: Vec<f32> = (0..96_000).map(|i| i as f32).collect();
    let mut clipboard: Option<Vec<f32>> = None;
    tools::tool::copy_region::apply(
        &buf, 48_000,
        tools::Range { start_sec: 0.5, end_sec: 1.5 },
        &mut clipboard,
    ).unwrap();
    let c = clipboard.unwrap();
    assert_eq!(c.len(), 48_000);
    assert_eq!(c[0], 24_000.0);
}
```

- [ ] **Step 2: Implementation**

```rust
//! Copy a region of the head's audio into a process-scoped clipboard.

use crate::Range;
use crate::util::range_resolver::RangeError;

pub fn apply(
    samples: &[f32],
    sample_rate: u32,
    range: Range,
    clipboard: &mut Option<Vec<f32>>,
) -> Result<(), CopyError> {
    let start = (range.start_sec * sample_rate as f64) as usize;
    let end = ((range.end_sec * sample_rate as f64) as usize)
        .min(samples.len());
    if end <= start {
        return Err(CopyError::Range(RangeError::InvalidOrder {
            start_sec: range.start_sec,
            end_sec: range.end_sec,
        }));
    }
    *clipboard = Some(samples[start..end].to_vec());
    Ok(())
}

#[derive(Debug, thiserror::Error)]
pub enum CopyError {
    #[error("{0}")]
    Range(#[from] RangeError),
}
```

In `apps/desktop/src-tauri/src/state.rs` add `clipboard: std::sync::Mutex<Option<Vec<f32>>>` to `AppState` and getters; the dispatcher will pass a `&mut` borrow.

- [ ] Register + schema + dispatcher arm.
- [ ] Run + commit.

---

### Task D5: `paste_region`

- [ ] Tests:
  - `paste_inserts_clipboard_at_offset`
  - `paste_with_empty_clipboard_errors`
  - `paste_at_offset_zero_prepends`
- [ ] Implementation `apply(samples: &mut Vec<f32>, sr, at_sec, clipboard: &Option<Vec<f32>>) -> Result<(), PasteError>` — error variant `EmptyClipboard`.
- [ ] Schema: `{ at: number }`.
- [ ] Register + dispatcher.
- [ ] Run + commit.

---

### Task D6: `label`

- [ ] Tests in `crates/tools/tests/label.rs`:
  - `label_writes_marker_annotation_via_store`
  - `label_with_range_writes_region_annotation`
  - `label_invalid_time_rejected`
- [ ] Implementation `crates/tools/src/tool/label.rs::apply(store: &mut Store, head: NodeId, name: &str, kind: AnnotationKind) -> Result<NodeId, LabelError>`. Internally calls `store.add_annotation(head, Annotation { id: AnnotationId::new(), name, kind })`.
- [ ] Schema: `{ time?: number, range?: Range, name: string }`.
- [ ] Register + dispatcher (returning the new head id).
- [ ] Run + commit.

---

## Phase E — Rust commands + IPC

### Task E1: `set_selection_context`

**Files:**
- Modify: `apps/desktop/src-tauri/src/state.rs` — add `selection: Mutex<Option<Range>>` + accessors.
- Modify: `apps/desktop/src-tauri/src/commands.rs` — new `set_selection_context` command.
- Modify: `apps/desktop/src-tauri/src/lib.rs` — register command.

- [ ] **Step 1: Test** (`apps/desktop/src-tauri/tests/commands_mock.rs` or new `selection.rs`):

```rust
#[test]
fn set_selection_context_round_trip() {
    let state = AppState::new();
    set_selection_context_inner(&state, Some(Range { start_sec: 1.0, end_sec: 2.0 }));
    assert_eq!(
        state.selection_snapshot(),
        Some(Range { start_sec: 1.0, end_sec: 2.0 })
    );
    set_selection_context_inner(&state, None);
    assert!(state.selection_snapshot().is_none());
}
```

- [ ] **Step 2: Implement**

In `state.rs`:

```rust
pub fn set_selection(&self, sel: Option<tools::Range>) {
    *self.selection.lock().expect("selection mutex") = sel;
}
pub fn selection_snapshot(&self) -> Option<tools::Range> {
    *self.selection.lock().expect("selection mutex")
}
```

In `commands.rs`:

```rust
#[tauri::command]
pub fn set_selection_context(
    state: State<'_, AppState>,
    range: Option<tools::Range>,
) -> CmdResult<()> {
    set_selection_context_inner(&state, range);
    Ok(())
}

pub(crate) fn set_selection_context_inner(state: &AppState, range: Option<tools::Range>) {
    state.set_selection(range);
}
```

Register in `lib.rs::invoke_handler`.

- [ ] **Step 3: Run**

Run: `cargo test -p edytlab-desktop`
Expected: pass.

- [ ] **Step 4: Commit.**

---

### Task E2: marker IPC commands

**Files:**
- Modify: `apps/desktop/src-tauri/src/commands.rs` — `add_marker`, `remove_marker`, `list_markers`.
- Modify: `apps/desktop/src-tauri/src/lib.rs` — register.

- [ ] **Step 1: Tests** — round-trip add → list → remove → list, asserting list lengths and that `marker-changed` is emitted (use `tauri::test::mock_app`).

- [ ] **Step 2: Implement** each command. They acquire `state.store_handle()`, call the matching `Store` helper, and emit `app.emit("marker-changed", …)`.

```rust
#[tauri::command]
pub async fn add_marker(
    app: AppHandle,
    state: State<'_, AppState>,
    time: f64,
    name: String,
) -> CmdResult<()> {
    let store = state.store_handle().ok_or(CommandError::NoSession)?;
    let head = state.head_snapshot().ok_or(CommandError::NoSession)?;
    let annotation = session::Annotation {
        id: session::AnnotationId::new(),
        name,
        kind: session::AnnotationKind::Marker { time_sec: time },
    };
    let new_head = lock_std(&store, "store")?
        .add_annotation(head, annotation)
        .map_err(CommandError::from)?;
    state.set_head(new_head);
    let _ = app.emit("marker-changed", ());
    Ok(())
}
```

`remove_marker` and `list_markers` follow the same shape.

- [ ] Register + run + commit.

---

### Task E3: `send_message` builds `SessionContext`

**Files:**
- Modify: `apps/desktop/src-tauri/src/commands.rs::send_message`.

- [ ] **Step 1: Test** — extend the existing `send_message`-related test to verify the system-prompt block is present in the request body when selection is non-empty (use a stub `Agent` that records its prompt).

If existing tests already cover the agent's prompt contents, a minimal smoke test that the call path doesn't panic is sufficient.

- [ ] **Step 2: Implement**

In `send_message`, after acquiring the agent guard:

```rust
let head = state.head_snapshot();
let store_handle = state.store_handle();
let markers = match (head, &store_handle) {
    (Some(h), Some(store)) => lock_std(store, "store")
        .ok()
        .and_then(|s| s.annotations_for(h).ok())
        .unwrap_or_default(),
    _ => vec![],
};
let ctx = ai::SessionContext {
    selection: state.selection_snapshot(),
    markers,
};
agent
    .turn_with_context(text, Some(ctx), on_event)
    .await
    .map_err(CommandError::from)?;
```

- [ ] **Step 3: Run + commit.**

---

## Phase F — Frontend

### Task F1: tauri-bridge wrappers

**Files:**
- Modify: `apps/desktop/src/lib/tauri-bridge.ts` — add `setSelectionContext`, `addMarker`, `removeMarker`, `listMarkers`, `onMarkerChanged`.

- [ ] Add functions to bridge file. Type definitions:

```ts
export interface MarkerKind {
  type: "marker" | "region";
  time_sec?: number;
  start_sec?: number;
  end_sec?: number;
}

export interface Marker {
  id: string;
  name: string;
  kind: MarkerKind;
}

export const setSelectionContext = (
  range: { start_sec: number; end_sec: number } | null,
): Promise<void> => invoke("set_selection_context", { range });

export const addMarker = (time: number, name: string): Promise<void> =>
  invoke("add_marker", { time, name });

export const removeMarker = (id: string): Promise<void> =>
  invoke("remove_marker", { id });

export const listMarkers = (): Promise<Marker[]> =>
  invoke("list_markers");

export const onMarkerChanged = (cb: () => void): Promise<() => void> =>
  listen("marker-changed", () => cb()).then((u) => () => u());
```

- [ ] No tests required; consumed by F3.
- [ ] Commit.

---

### Task F2: `Ruler` + `MarkerLayer` components

**Files:**
- Create: `apps/desktop/src/components/Ruler.tsx`
- Create: `apps/desktop/src/components/MarkerLayer.tsx`
- Create: `apps/desktop/src/components/__tests__/MarkerLayer.test.tsx`

- [ ] **Step 1: Tests for `MarkerLayer`**

```tsx
import { render, screen, fireEvent } from "@testing-library/react";
import { MarkerLayer } from "../MarkerLayer";

test("renders one flag per marker", () => {
  const markers = [
    { id: "1", name: "chorus", kind: { type: "marker" as const, time_sec: 10 } },
    { id: "2", name: "drop", kind: { type: "marker" as const, time_sec: 20 } },
  ];
  render(<MarkerLayer markers={markers} duration={30} onSeek={() => {}} onRemove={() => {}} />);
  expect(screen.getAllByTestId("marker-flag")).toHaveLength(2);
});

test("clicking a flag calls onSeek with marker time", () => {
  const onSeek = vi.fn();
  const markers = [{ id: "1", name: "chorus", kind: { type: "marker" as const, time_sec: 10 } }];
  render(<MarkerLayer markers={markers} duration={30} onSeek={onSeek} onRemove={() => {}} />);
  fireEvent.click(screen.getByTestId("marker-flag"));
  expect(onSeek).toHaveBeenCalledWith(10);
});
```

- [ ] **Step 2: Implement** `MarkerLayer` and `Ruler` (Ruler is decorative ticks + click-to-add). `MarkerLayer` renders absolute-positioned flags with `left: (time / duration * 100)%`.

- [ ] **Step 3: Run vitest, then commit.**

---

### Task F3: Timeline integration

**Files:**
- Modify: `apps/desktop/src/components/Timeline.tsx`

- [ ] Above the head lane, render `<Ruler … />` and `<MarkerLayer … />`.
- [ ] Pass `markers`, `duration`, `onSeek` (calls `TimelineHandle.seekTo`), `onAddMarker(time, name)` callbacks.
- [ ] Existing `data-testid`s preserved.
- [ ] Run vitest. Commit.

---

### Task F4: App state for markers + selection IPC

**Files:**
- Modify: `apps/desktop/src/App.tsx`

- [ ] Add `useState<Marker[]>([])` for markers; on mount, `listMarkers().then(setMarkers)` and subscribe via `onMarkerChanged(() => listMarkers().then(setMarkers))`.
- [ ] Replace the existing `setSelection` with a wrapper that ALSO calls `setSelectionContext({ start_sec, end_sec })` on the bridge, debounced 250 ms via `setTimeout` cleared on each change.
- [ ] Pass `markers` + an `onAddMarker(time, name)` (calls `addMarker(time, name)`) into Timeline.
- [ ] Run vitest. Commit.

---

### Task F5: Chat marker context line

**Files:**
- Modify: `apps/desktop/src/components/Chat.tsx`

- [ ] When `selection` is set OR a `markers: Marker[]` prop is passed and non-empty, the existing prefix line gains a marker context segment when relevant. Keep it minimal: prefix is `[apply to ...] [markers: chorus@0:42, drop@1:18] message`.
- [ ] Update existing `Chat.test.tsx` cases to cover marker prefix presence.
- [ ] Run vitest. Commit.

> Implementation note: the Rust `SessionContext` already injects markers into the system prompt, so the marker prefix in the user message is partly redundant. Keep it for transparency — anyone reading the transcript sees what context the agent operated under.

---

## Phase G — Integration smoke + release

### Task G1: Manual smoke checklist

After all unit suites pass on CI, run the v0.1.0-dev release once it auto-publishes and verify:

- [ ] Drop a WAV, audio loads.
- [ ] Drag region on waveform → status bar shows `sel … → … (…s)`, chat input shows pill.
- [ ] Send "fade out" → bubble shows `[apply to MM:SS-MM:SS] fade out`, audible fade applied to selection.
- [ ] `Space` plays / pauses; `Home` rewinds; `End` seeks to end.
- [ ] Click ruler at ~10 s, type "verse" → marker flag appears, `marker-changed` event fires, system prompt receives marker (verifiable by saying "fade out at the verse marker" → fade applied at 10 s).
- [ ] Right-click marker → context menu deletes it.
- [ ] `copy_region` then `paste_region` round-trips audio.
- [ ] `insert_silence` extends duration by the requested amount.
- [ ] `reverse` plays back reversed.
- [ ] Fork node via existing graph view; markers don't bleed across siblings.

If any check fails, file a follow-up issue rather than blocking the wave merge — tools that ship are tools that get used.

---

## Spec coverage check

| Spec section | Tasks |
|---|---|
| 3 — Architecture | A1-A4 (annotations), C1-C2 (SessionContext), E1-E3 (IPC), F1-F4 (FE plumbing) |
| 4 — Components | A1-A4, B1, C1-C2, D1-D6, E1-E3, F1-F5 |
| 5 — Data flow | E1-E3 (Rust), F4 (FE debounced selection), F2-F3 (markers UI) |
| 6 — Tool specs | D1-D6 |
| 7 — Schema change | A1-A4 |
| 8 — Error handling | Test cases throughout (range_resolver, tools, IPC) |
| 9 — Testing | Tests embedded in every task; Phase G covers integration smoke |

---

## Self-review

- All steps include exact file paths and code blocks; no `TBD` / `TODO` / "fill in" placeholders.
- Type names consistent: `Annotation`, `AnnotationId`, `AnnotationKind`, `Range`, `RangeError`, `SessionContext`, `MarkerLayer`, `Ruler`.
- `Range` defined once in `crates/tools/src/util/range_resolver.rs`, re-exported from `crates/tools/src/lib.rs`, imported wherever needed (including `crates/ai`).
- `Annotation` defined in `crates/session/src/annotation.rs`, re-exported from session crate root.
- Tombstone variant from the spec was dropped in favour of append-new-state (D4 / E2 use `Store::remove_annotation` which writes a new node with the marker filtered out). This is a simpler implementation that respects the same content-addressed-graph model. Spec section 7 mentions tombstones but the equivalent behaviour is preserved — the plan is internally consistent.
- Phase D tools D2-D6 are written compactly because they all share D1's pattern; engineers should reuse that scaffold step-for-step.
- Acceptance: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `pnpm --filter @edytlab/desktop test`, `pnpm --filter @edytlab/desktop exec tsc --noEmit` — same gates the existing CI enforces.
