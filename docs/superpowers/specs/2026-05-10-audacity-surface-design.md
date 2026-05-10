# Audacity-feature surface + first wave — design

**Status**: design (pre-implementation)
**Author**: Claude (session 01K4sSA1pTm2ixfPSSQk45Ws), human approval pending
**Date**: 2026-05-10
**Scope**: foundation surface for region-aware tools + markers, plus a five-tool reference wave (`fade`, `reverse`, `insert_silence`, `copy_region`, `paste_region`) and the marker primitive `label`.

## 1. Goal & non-goals

### Goal

Bring edytlab toward Audacity-class editing capability where every feature is reachable through the agentic chat interface. This document establishes the **surface** — how region selections and markers reach agent tools, where markers live, how typed tool params combine with natural-language prefixes — and lands a small representative **wave** of tools that proves the surface end-to-end.

### Non-goals

- Full Audacity feature parity in this spec. Future tools (EQ, compression, noise reduction, spectrogram view, recording, MP3 export, plugin host, etc.) plug into the surface in subsequent specs.
- Live recording / monitoring. Out of scope.
- Plugin host (LADSPA / VST). Out of scope.
- Per-stem playback / mixing. Stays a Phase 3 concern.
- UI editing of session-graph nodes. Markers are the only annotation users edit directly.

## 2. Use-case constraint

**Generalist intersection** — the wave is the five operations every podcast / music / DJ workflow needs: fade, copy + paste region, insert silence, label, reverse. Specialist tools (BPM-sync, EQ, compressor, noise reduction) queue behind this in their own specs.

## 3. Architecture

```
┌─ Frontend (React) ───────────────────────────────────────────┐
│  Timeline      ◀──ws regions──▶ MarkerLayer (ruler flags)    │
│   │                                  │                       │
│   ▼ selection                        ▼ marker CRUD           │
│  App state ────────────┬─────────────┘                       │
│                        │                                     │
│  Chat ◀── prefix ──────┤  bridgeSendMessage("[apply to       │
│                        │   0:23-0:45] [marker chorus@0:42]   │
│                        │   fade out")                        │
└────────────────────────┼─────────────────────────────────────┘
                         ▼ IPC
┌─ Rust (apps/desktop) ──┴─────────────────────────────────────┐
│  commands::send_message                                      │
│   │                                                          │
│   ▼  builds SessionContext { selection, markers, head }      │
│  ai::agent_loop                                              │
│   │  system prompt += "selection: 0:23-0:45 / markers: …"    │
│   ▼                                                          │
│  tools::dispatcher  ─▶ tool::fade { range: 0:23-0:45, … }    │
│   │                                                          │
│   ▼                                                          │
│  session::Store  ◀─ writes Edit node + Annotation node       │
└──────────────────────────────────────────────────────────────┘
```

### Hybrid plumbing rationale

- **Text prefix** (`[apply to MM:SS-MM:SS]`) is what the LLM sees in the user message. Keeps the conversation transparent and self-contained — anyone reading a transcript can see what was acted on.
- **Typed tool params** (`range: { start, end }`) are what the tool dispatcher consumes. Prevents parsing errors from breaking edits.
- Markers are read-only context: the agent sees them in the system prompt and resolves marker names to times when filling `range`. No tool in this wave takes a `marker_id` param — `label` is the only writer, and it takes `time` directly.
- The agent system prompt receives the canonical `SessionContext` JSON each turn; tools should pull from that when typed params are missing.
- `range_resolver` precedence: typed `range` param > parsed text-prefix > error.

### Markers as session-graph nodes (architecture B)

Single source of truth: the existing `session::Store`. Markers are a new `Annotation` node variant. Annotations on a parent are visible at any descendant head; revert / fork move users to a different annotation set automatically — same model as the rest of the graph. No parallel marker store.

## 4. Components

### Frontend (`apps/desktop/src/`)

- **Timeline.tsx** — `TimelineHandle` gains `setMarker(time, name)`, `removeMarker(id)`. Mousedown on a marker drags it. Right-click → context menu (rename, delete).
- **components/MarkerLayer.tsx** *(new)* — absolute-positioned ruler above the head lane. Renders flags from `markers` prop; click jumps the playhead via `TimelineHandle.seekTo`.
- **components/Ruler.tsx** *(new)* — 5-second tick ruler above MarkerLayer. Click on the ruler creates a marker at click time (inline name prompt).
- **lib/tauri-bridge.ts** — new wrappers `addMarker`, `removeMarker`, `listMarkers`, `setSelectionContext` (last is debounced 250 ms).
- **App.tsx** — new state `markers: Marker[]`; subscribes to `marker-changed` event for sync; passes selection + markers to Chat for prefix construction.

### Rust (`apps/desktop/src-tauri/`)

- **commands.rs** — new commands `add_marker`, `remove_marker`, `list_markers`, `set_selection_context`. `send_message` builds `SessionContext` from frontend-pushed selection + store-loaded markers and threads it into the agent loop.

### `ai` crate (`crates/ai/`)

- New `SessionContext { selection: Option<Range>, markers: Vec<Marker>, head: NodeId }` struct passed into `agent_loop`.
- System-prompt builder appends a deterministic block when context is non-empty:
  ```
  ## current_selection
  start: 23.45s  end: 45.10s

  ## markers
  - chorus @ 42.00s
  - drop   @ 78.50s
  ```
- Empty selection / empty markers → block omitted entirely.

### `session` crate (`crates/session/`)

- New `Node::Annotation { parent: NodeId, kind: AnnotationKind, name: String, created_at: SystemTime }` variant.
- `AnnotationKind = Marker { time_sec: f64 } | Region { start_sec: f64, end_sec: f64 } | Tombstone { target: NodeId }`.
- New `Store` API: `add_annotation`, `remove_annotation` (writes a `Tombstone`), `annotations_for(head)` (walks parent chain, applies tombstones, sorts by time).

### `tools` crate (`crates/tools/src/tool/`)

- `fade.rs`, `reverse.rs`, `insert_silence.rs`, `copy_region.rs`, `paste_region.rs`, `label.rs`.
- Shared helper `tools::util::range_resolver` (typed param > text-prefix > error).
- Schemas in `tools::schema` declare a structured `range?: Range` type that any region-aware tool reuses.

## 5. Data flow

### Selection round-trip (drag → tool)

1. User drags region on Timeline lane → `onSelectionChange({ start, end })` in App.
2. App debounces 250 ms → `setSelectionContext({ start, end })` IPC.
3. Rust stores selection in `AppState.selection` (ephemeral, not persisted).
4. User submits chat: text prefix `[apply to 0:23-0:45]` plus typed param the agent fills.
5. `send_message` reads `AppState.selection` + walks store annotations → builds `SessionContext`.
6. `agent_loop` injects context into system prompt.
7. Agent calls e.g. `fade(range: { start: 23, end: 45 }, kind: "out")` — typed param wins; if missing, `range_resolver` parses prefix.
8. `tools::fade` resolves `Range` against the current head's audio → writes a new Edit node → emits `node-created`.
9. Frontend re-renders waveform from the new node; selection cleared (new audio = stale region).

### Marker lifecycle (UI add)

1. User clicks ruler at t=42s, types "chorus" → `addMarker(t, "chorus")` IPC.
2. Rust calls `Store::add_annotation(parent: head, kind: Marker { time_sec: 42 }, name: "chorus")`.
3. Annotation node committed; `marker-changed` event emitted.
4. Frontend listens, re-fetches `listMarkers()` → MarkerLayer re-renders.
5. Next chat turn: `SessionContext.markers` includes the new marker; agent can reference "chorus" by name.

### Marker lifecycle (agent add)

1. User: "mark the chorus around 42 seconds".
2. Agent calls `label(time: 42, name: "chorus")` — same `Store::add_annotation` path as the UI add.
3. `marker-changed` event flows to frontend identically.

### Edits at marker

1. User: "fade out starting at the chorus marker".
2. Agent reads `markers["chorus"].time = 42`, calls `fade(range: { start: 42, end: duration }, kind: "out")`.
3. Same node-creation path; the marker remains pointing at t=42 on the new edit node.

## 6. Tool specs (wave)

### `fade`
- **Params**: `range: Range` (required), `kind: "in" | "out"`, `curve: "linear" | "exponential"` (default linear).
- **Audio op**: multiply samples in `range` by a ramp 0→1 (in) or 1→0 (out).
- **Edit semantics**: new Edit node referencing parent head; payload is a WAV diff over `range`.

### `reverse`
- **Params**: `range: Option<Range>` (whole track if `None`).
- **Audio op**: reverse sample order in `range`, splice back.
- **Edit semantics**: same as `fade`.

### `insert_silence`
- **Params**: `at: f64` (seconds), `duration: f64`.
- **Audio op**: open existing WAV, splice `duration * sample_rate` zero samples at `at`. Net length increases.
- **Edit semantics**: new Edit node, parent diff is "insert N samples at offset M".

### `copy_region`
- **Params**: `range: Range`.
- **Effect**: copies `range` of head's audio into a process-scoped clipboard (`AppState.clipboard: Option<AudioBuffer>`).
- **Edit semantics**: NO new node — pure read.

### `paste_region`
- **Params**: `at: f64`.
- **Audio op**: clipboard inserted at `at` (errors if clipboard empty).
- **Edit semantics**: new Edit node.

### `label` (marker primitive)
- **Params**: `time: f64`, `name: String`, OR `range: Range` + `name`.
- **Effect**: writes an `Annotation` node into the session store. NOT an audio edit.

### Shared
- `range_resolver` lives in `tools::util`.
- Standard `CmdResult` errors: `InvalidRange`, `RangeOutOfBounds`, `NoHead`, `EmptyClipboard`, `MissingRange`, `InvalidParam`.
- All tools register their schema in `tools::schema` with the shared `range` type so the agent's tool catalogue documents the contract.

## 7. Schema change to `session` crate

```rust
// crates/session/src/node.rs (illustrative)
pub enum Node {
    Project { ... },
    Edit { parent: NodeId, op: EditOp, audio_hash: Hash },
    // …existing variants
    Annotation {
        parent: NodeId,
        kind: AnnotationKind,
        name: String,
        created_at: SystemTime,
    },
}

pub enum AnnotationKind {
    Marker { time_sec: f64 },
    Region { start_sec: f64, end_sec: f64 },
    Tombstone { target: NodeId },
}
```

- **Persistence**: same content-addressed storage as Edit nodes. Annotation `NodeId` = blake3 of its serialised payload.
- **Walking semantics**: `Store::annotations_for(head)` walks the parent chain, collects every Annotation whose `parent` is an ancestor of (or equal to) `head`, applies `Tombstone` removals, sorts by time. Annotations on a sibling branch are NOT visible.
- **Migration**: existing stores have no Annotation nodes; readers see an empty marker set. The format is append-only — no on-disk migration.

## 8. Error handling

| Boundary | Failure mode | Behaviour |
| --- | --- | --- |
| FE selection drag with `duration == 0` | audio not loaded yet | no-op, no IPC fired |
| FE marker click on ruler with no audio | no head | button disabled |
| FE IPC `setSelectionContext` failure | transient | silent — debounced retry on next change |
| FE IPC `addMarker` failure | persistent | inline `ErrorBanner` |
| Rust `set_selection_context` with out-of-range | LLM hallucinated coordinates | clamp + warn-log; advisory only |
| Rust `add_marker` `time < 0` or `> duration` | invalid input | `CommandError::InvalidRange` |
| Rust `remove_marker` for unknown id | idempotent | success no-op |
| Tool: neither typed param nor parseable prefix | missing range | `MissingRange` returned to agent so it can re-prompt |
| Tool: `paste_region` with empty clipboard | UX gap | `EmptyClipboard` so agent can suggest "copy first" |
| Tool: `insert_silence` with `duration < 0` | invalid input | `InvalidParam` |
| Tool: range outside head duration | invalid input | `RangeOutOfBounds` with actual duration in message |
| Agent loop tool error | non-fatal | error string passed to agent; can retry / re-prompt / pick another tool |
| Store lock poisoned | infrastructure | `CommandError::SessionUnavailable`; chat shows inline error banner |

## 9. Testing

### `crates/session/tests/annotations.rs` *(new)*
- `add_marker_visible_at_head`
- `marker_on_parent_visible_at_descendant_head`
- `marker_on_sibling_branch_not_visible`
- `tombstone_hides_marker`
- `tombstone_idempotent`
- `region_annotation_round_trip`

### `crates/tools/tests/` *(per-tool)*
- `fade_in_applies_ramp`, `fade_out_applies_ramp`, `fade_outside_range_unchanged`
- `reverse_full_track`, `reverse_subrange_only`
- `insert_silence_extends_duration`, `insert_silence_negative_rejected`
- `copy_then_paste_round_trips`, `paste_without_copy_errors`
- `label_creates_annotation_node`
- `range_resolver_typed_param_wins`, `range_resolver_falls_back_to_text_prefix`, `range_resolver_missing_errors`

### `crates/ai/tests/session_context.rs` *(new)*
- `system_prompt_includes_selection`
- `system_prompt_includes_markers_sorted_by_time`
- `system_prompt_omits_block_when_empty`

### `apps/desktop/src-tauri/tests/`
- `set_selection_context_roundtrip`
- `add_remove_marker_via_ipc`
- `marker_changed_event_emitted_after_add`

### `apps/desktop/src/components/__tests__/MarkerLayer.test.tsx` *(new, vitest)*
- Renders flag for each marker with correct `left: %` based on duration.
- Click on flag calls `onSeek` with marker time.
- Right-click opens context menu.

### `apps/desktop/src/__tests__/Chat.test.tsx` *(extend existing)*
- Submits message with `[apply to ...]` prefix when selection active.
- Submits raw message when selection null.

### Integration smoke (manual on dev build)
- Drop WAV → drag region → "fade out" → audible fade verified.
- Click ruler → marker appears → say "fade out at chorus" → fade applied at marker time.
- Fork node → markers from old branch hidden → revert → markers re-appear.

CI already enforces the unit suites; the integration smoke is a release-gate manual check until full E2E lands.

## 10. Out of scope (queued for future specs)

- Effects: EQ, compressor, reverb, echo, paulstretch, vocoder, change-speed, repeat, invert, bass / treble.
- Analysis: spectrogram view, plot spectrum, find clipping, full beat detection.
- Restoration: noise reduction, click / pop removal.
- Recording / live monitoring.
- Multi-format export (mp3, ogg, flac).
- Envelope tool / volume automation curves.
- Time-shift between tracks.
- Spectral selection.
- LADSPA / VST plugin host.
- Audacity macros / chains (the agent IS the macro chain — but a "save this conversation as a reusable macro" feature is a separate spec).
