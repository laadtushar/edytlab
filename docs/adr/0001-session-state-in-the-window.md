---
title: "0001. The window changes the session through one module, built on TanStack Query"
sidebarTitle: "0001 Session state in the window"
---

Status: Accepted, 2026-10-22.

## Context

The session lives in the Rust backend; the window holds a copy of parts of it (the head, the path
taken, the track list, the graph, the markers) and changes it through Tauri commands. Every handler
in `App.tsx` that changes the session assembles the same steps by hand: queue the call
(`inOrder`), call the command, adopt the head, mark the mix stale, hand the user's held lane
controls a claim or release them, run a numbered track read, and for moves skip undo steps a
compaction removed. Sixteen handlers do some subset of this; the rules are held by two regex scans
over `App.tsx` and three CLAUDE.md bullets. Bugs of this shape: #596, #602, #595, #522, #498, #232,
and Accept B (no track re-read, no release of held controls after it moves the head).

Each piece we built by hand is a known problem with a standard answer:

| Ours | The general problem | TanStack Query v5 |
|---|---|---|
| `lib/sessionQueue.ts` (`inOrder`) | run writes one at a time | mutations with the same `scope.id` "will run in serial" |
| numbered `refreshTracks` | a read requested before a write must not land after it | `cancelQueries` before a write, "so they don't overwrite our optimistic update" |
| `ownedControls` | show the user's value until the server confirms it | optimistic updates through the mutation's `variables`, released on settle |
| `graphVersion` and per-view re-reads | re-read what a write may have changed | `invalidateQueries` on settle ("always refetch after error or success") |

React's documentation says these problems are "not trivial", recommends a framework's built-in
data fetching, and otherwise a custom Hook so you can "adopt an efficient data fetching strategy
later"; a desktop app built with Vite has no such framework, so the Hook (`useSession`) plus a
library behind it is the path it describes.

## Decision

1. **One module.** `useSession` grows into the only way the window changes the session. Its
   interface has two operations:
   - `edit(command, options)`: a command that returns a new head (track gain, a marker, a template,
     a cut). The user's held lane controls keep their claims until a read after the edit lands.
   - `move(kind)`: undo, redo, Set as head, Accept B, open a project, and the assistant's
     `agent://node-created`. The session moved by itself: held lane controls are released, the
     track list is re-read, and undo/redo skip steps a compaction removed (#595).
   The head, the path taken (#398, #453) and the undo rule stay in the window, as #568 decided.
2. **TanStack Query** (`@tanstack/react-query` 5.104.1, MIT, React 18 or 19) is the server-state
   layer behind that module:
   - reads (`tracks`, `graph`, `markers`, `transcript`, render warnings) are queries keyed under
     `['session', …]`;
   - session-changing commands are mutations in one scope, `'session'`, so they run in order, which
     replaces `sessionQueue.ts`;
   - each mutation cancels in-flight reads before it runs and invalidates the reads it can change
     when it settles, from one table in the module, which replaces `graphVersion` and the numbered
     reads;
   - a held lane control is the mutation's `variables` (and the slider's input value before the
     commit), shown over the query data until the invalidation after the commit has landed; this
     keeps #596's rule with the library's mechanism instead of `ownedControls.ts`.
3. **Structural guard.** The session-changing bridge commands move to `lib/sessionCommands.ts`,
   which only the module may import, checked by one vitest import test (the app has no ESLint).
   `headIsApplied.test.ts` and `sessionQueueCoverage.test.ts` are deleted once it holds.
4. **The mix is stale by comparison only.** `mixIsStale` compares the mix's node with the node
   shown; no handler clears the mix by hand.
5. **Edits during the assistant's turn** stay allowed, as today (the backend's store lock orders
   them); the module records the turn's start and end so a move made by the assistant releases held
   controls like any other move.

## Order of work

1. A fix PR first, with a failing test: Accept B releases held controls and re-reads the tracks.
2. Add the provider and move the track list to a query, with held controls on it (behind the same
   `useSession` interface); e2e `lane-readback`, `undo-path`, `labels`, `mix-evicted` must pass.
3. Commands to mutations in the `'session'` scope; `edit`/`move`; the import guard; delete the scans.
4. The other reads (graph, markers, transcript, warnings) to queries; delete `graphVersion`.

## Considered and rejected

- **Deepen `useSession` by hand, no library.** Keeps zero new dependencies, but keeps four
  mechanisms we maintain ourselves for problems the library solves and tests in many more apps.
- **A global store (Redux Toolkit, Zustand).** They manage client state; they do not handle
  cancellation, invalidation or serial writes against a server.
- **A state machine (XState) for the whole session.** Strong for the head/path rules, heavy for
  reads and invalidation; can be added later inside the module if the undo rules grow.
- **Moving the undo rule into the backend.** #568 decided against it until a second client exists.

## How we will know

The two regex scans are gone and an import test holds the rule; a new session-changing command is
one entry in the module; the e2e lane, undo, labels and mix specs pass unchanged; Accept B and the
assistant's edits follow the move rules with tests.

Sources: https://tanstack.com/query/latest/docs/framework/react/guides/mutations (mutation scope),
https://tanstack.com/query/latest/docs/framework/react/guides/optimistic-updates (cancel, rollback,
invalidate), https://react.dev/learn/you-might-not-need-an-effect#fetching-data (read 2026-10-22).
