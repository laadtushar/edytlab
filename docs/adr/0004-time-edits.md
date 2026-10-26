---
title: "0004. A time edit is a pure function over the session state"
sidebarTitle: "0004 Time edits"
---

Status: Accepted, 2026-10-22.

## Context

An edit that shifts time can move four things: the edited track's clips, the other tracks (when
sync-lock is on), the labels, and the transcript words. Nine tools each pick their own subset of
helpers (`sync_other_tracks`, `remap_after_cut/insert/scale`, `cut_annotations`), and the fixes came
one tool at a time (#203, #275, #278, #543, #572/#586).

## Decision

A module in `crates/tools`, `time_edit`, with one pure function (a "functional core"):

    apply(state: &SessionState, edit: TimeEdit, scope: Scope) -> (SessionState, TimeEditReport)

- `TimeEdit` is `Cut { start, end }`, `Gap { at, len }` or `Scale { factor }`.
- `Scope` says which track was edited and whether the tool follows sync-lock; whether a tool follows
  sync-lock becomes a declared property of the tool, not code it may forget.
- It moves clips, the other tracks when the scope says so, labels and words, and returns what moved
  and what was dropped (`synced_tracks`, `dropped_labels`), the same fields everywhere.
- The nine tools state their edit and call it; destructive audio processing (`truncate_silence`,
  `time_stretch`) stays in the tool, only the time change moves.
- Which tools follow sync-lock does not change in this work: widening it is a product decision.

## How we will know

Property tests (`proptest`) on `apply`: a marker after a cut moves by the cut's length, a clip that
straddles a gap is split, a scale followed by its inverse is the identity within rounding, nothing
before an edit moves. Per-tool tests keep only what is specific to the tool. Node ids hash state, so
a change to what moves changes new edits' ids only; old nodes replay unchanged (`rederive`).

Sources: https://proptest-rs.github.io/proptest/ (property testing for Rust).
