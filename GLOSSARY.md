---
title: "Glossary"
---

Terms the code, the docs and the architecture records use with one meaning each. Add a term when a
module is named after it; sharpen a definition when it turns out to be fuzzy.

**Session.** The project's history as a graph of nodes, each a full state (tracks, clips, effects,
markers, transcript), held by the backend's store (`crates/session`).

**Node.** One state in the session graph. Its id hashes the state, so a state reached twice is one
node (#398).

**Head.** The node the window shows and edits from.

**Path (path taken).** The sequence of heads the user actually moved through, which undo and redo
follow; not the graph's parent links (#398, #453, `lib/headTrail.ts`).

**Edit.** A change the user asks for that creates a new node and moves the head to it (a gain, a
marker, a cut). Contrast **move**.

**Move.** A change of head the session makes without a new edit from the user: undo, redo, Set as
head, Accept B, opening a project, an assistant edit. After a move the window releases held
controls and re-reads the session (ADR 0001).

**Held control (owned control).** A lane control (gain, pan, mute, solo) whose value the user set and
the session has not yet confirmed; the window shows the user's value until a read after the commit
lands (#596).

**Line.** The order in which the window's session-changing calls run, one at a time (today
`inOrder`; with ADR 0001, the mutation scope `'session'`).

**Preview cache.** The bounded folder of rendered mixes (`previews/`); an entry can be **held** so
eviction skips it while the window needs it (#607).

**Left-out effect.** A stored effect the renderer could not run and so left out of a render, which
every render must report (#520, #531; ADR 0003).

**Plan gate.** Plan first's wait for exactly one answer to a shown plan: approve, revise, reject or
time out (ADR 0002).

**Time edit.** A cut, a gap or a scale of the timeline, with what it moves: clips, other tracks
under sync-lock, labels, words (ADR 0004).

**Live store.** The one `Store` per project folder, found by the folder's identity (#566).

**Derived file.** Audio the project wrote under a name made from its samples, in
`.audiograph/derived/` (#620; ADR 0008).
