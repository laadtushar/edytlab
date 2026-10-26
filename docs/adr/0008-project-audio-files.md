---
title: "0008. The project's own audio files are one content-addressed store"
sidebarTitle: "0008 Project audio files"
---

Status: Accepted, deferred until the stem writers (#385) need it, or a quiet period with no
data-loss fixes in flight.

## Context

"Which files does this project own, under what name, and when may one be deleted?" is answered in
two crates and about nine modules: folder names in `session::relocate`, naming in
`provenance::audio_hash` (except `mix_to_new_track`, which hashes its WAV), writing in `util.rs`,
resolving in the store's `Rebinder`, deleting in three sweeps plus compaction (each re-checking
"inside `derived/`" and the in-flight count), copying in Save As and repair. Bugs: #355, #368, #371,
#372, #376, #378-#382, #571, #580/#588, #589/#593, #612/#616, #620/#632.

## Decision

One module in `crates/session`, beside `relocate` and the live-store registry, is the project's
content-addressed store (the model git uses for its objects): name new audio (and recognise the
pre-#620 names), write without replacing, resolve a clip's file, say whether a path is the project's
to delete, and delete only while nothing is staged (mark from the nodes that name a file, then
sweep). `reclaim`, `rederive`, `repair`, compaction and Save As become its callers; writers cannot
name files themselves, so the source scan for `blake3::` in tool code is no longer needed.

Kept, from #620: names are compared and never recomputed from a file; the older-name match stays;
no scheme field is added to stored ops. Kept, from #566: the in-flight guard.

## How we will know

Ownership and legacy-name cases are tested once in the module instead of across
`legacy_audio_names.rs`, `compaction_owned_files.rs`, `copied_project.rs` and `reclaim.rs`; the stem
files of #385 are written through it from day one.
