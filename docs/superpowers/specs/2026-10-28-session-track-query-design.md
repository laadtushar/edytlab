# Session track query: ADR 0001 Stage 2

Issue: [#713](https://github.com/laadtushar/edytlab/issues/713).
Authority: accepted [ADR 0001](../../adr/0001-session-state-in-the-window.md).
Baseline: `60ca7a17074dc624392c4c747ad67b9143468ec6`.
Status: narrow scope approved in chat on 2026-10-28; written design awaiting
review before an implementation plan or source changes.

## Outcome and scope

Move the current track-list read and stale-read protection from `App` behind
`useSession`, backed by a real per-app TanStack Query client. Preserve what the
user sees and every current control/command ordering rule. This completes only
Stage 2, not all of #713; the eventual PR uses `Refs #713`.

Two approaches were considered. The selected approach migrates reads while
keeping the existing `OwnedControls` instance and command handlers in `App`.
Moving control input, commit deduplication, and ownership behind a second large
interim interface would expand the migration and then be replaced by Stage 3.
The earlier ten-method forwarding interface is rejected. No new store or custom
query framework is introduced.

Out of scope: mutations, `edit`/`move`, replacing `sessionQueue`, deleting regex
guards, replacing `OwnedControls`, deleting `graphVersion`, other session
queries, mix-staleness redesign, native/IPC changes, and any new outbound host.

## Module boundary

`useSession` retains all existing head, path, queue, and graph methods. Its result
adds `tracks` and `refreshTracks`. Query keys, result envelopes, cache operations,
read tickets, and cancellation stay private to the session module.

The no-argument hook remains usable by history-only callers. Overloads distinguish
an owner-aware call from a call without control ownership:

```ts
useSession(); // tracks; refreshTracks(): Promise<void>; existing fields
useSession({ trackControls: owned });
// tracks; refreshTracks(claim?: Claim): Promise<void>; existing fields
```

Passing a claim without its owner must fail type checking. `Claim` and
`OwnedControls` are existing concepts, not a new generic plugin interface.

`App` constructs its existing owner before calling the hook. It retains
`set`/`send`/`sent`/`skip`/`clear`/`overlay`, stable track keys, lane mapping,
`ownedVersion`, and the current `tracksRef` passive-effect timing. Only the local
track state, read counter, and numbered refresh implementation leave `App`.
`commitTrackChange` calls `refreshTracks(claim)` instead of explicitly finishing
the claim with App's counter. Other callers keep `refreshTracks()`.

An internal track-query hook may contain this read logic; it is not another
component-facing control API. Comments describing the old numbered reads move
with their responsibility rather than remaining on obsolete App code.

## Provider and query configuration

Pin `@tanstack/react-query` to **5.104.1**, as named by the ADR. npm records its
publication on 2026-10-02; it exceeds the issue's fourteen-day minimum and depends
exactly on `@tanstack/query-core@5.104.1`. Both are MIT and support the repo's React
version. Generate the required third-party notices; do not hand-edit them.

A small provider owns one lazily created real QueryClient per mounted App.
Default `App` wraps its inner implementation with this provider, so the existing
acceptance tests that mount `<App/>` remain unchanged. `main.tsx` needs no second
provider. No global singleton or implicit test-only fallback is allowed.
Hook-only fixtures receive an explicit fresh provider/client per test.

The track key is private under `['session', 'tracks']`. `listTracks` reads the
currently open backend project, not an arbitrary node; do not label its result
with a head it did not actually read. A project reset supersedes old reads even
when head and track IDs match the previous project.

Use a disabled query observer and retain the existing App boot refresh as the
single boot mechanism. StrictMode may replay that mechanism; obsolete replies
must still be harmless. Local IPC uses `networkMode: 'always'`, `retry: false`,
and no focus/reconnect refetches. An explicit refresh always starts a fresh read,
not a cache hit. Do not add a head-driven automatic fetch beside current refresh
callers.

## Refresh, cancellation, and visible adoption

Each refresh follows this sequence:

1. Synchronously allocate a monotonically increasing private ticket. If a claim
   is supplied, call its owner's `finish(claim, ticket)` before the first await.
2. Explicitly cancel the exact track query, then verify this refresh is still
   current before starting a new read. Do not rely on invalidation alone: this
   release can reuse a pending initial no-data promise despite `cancelRefetch`.
3. The query function only reads the bridge and returns a private successful
   snapshot `{ ticket, tracks }`. It has no ownership or React-state side effects.
   Preserve the existing `isNoSession` recognition and convert that condition to
   a successful empty list. Other current-read errors reject to the caller;
   superseded errors are ignored. Boot retains its quiet failure behavior.
4. Only the current successful snapshot may confirm a claim. Return its tracks
   through the query observer. In a layout effect associated with that adopted
   snapshot, call `owned.applied(ticket)`, so the overlay is not released before
   the confirming list is renderable. A failed, cancelled, or obsolete read
   releases nothing. A ticket distinguishes successful confirmations even when
   structural sharing reuses an identical track array.
5. Preserve newer unsent inputs and queued commit deduplication, including the
   `0 -> -3 -> 0` case. The current owner supplies these rules unchanged.

The refresh promise means the current read settled, not that React has painted
or a layout effect has run. Consumers and tests must wait for observed state,
not assume one awaited promise proves a visible update. Existing command queue
turns and whether a refresh occurs inside or outside them stay unchanged.

`resetHead` keeps its void interface. Before adopting the reset it privately
supersedes pending tickets and cancels the exact query, without resetting the
monotonic ticket counter. App's current release-before-refresh paths remain for
undo, redo, Set as head, Accept B, project opening, assistant nodes, and punch.
Preview clicks remain separate from head moves.

The track-query hook is the one track-query owner per App. Its effect cleanup
explicitly advances a private observer-lifetime generation, supersedes pending
read tickets, and cancels the exact query. Each refresh captures that lifetime
and checks it as well as its ticket after awaiting cancellation and before
confirming a snapshot. StrictMode setup starts an active lifetime without
resetting the ticket counter; merely toggling a mounted boolean is insufficient
because an older refresh could resume after reattachment. Do not rely on the
library's automatic final-observer cancellation or consume its AbortSignal as
an implicit cancellation mechanism. Test a held native read resolving after
the final observer unmounts: it must not repopulate the cancelled query or
confirm ownership. A new mounted App/client still starts its own normal read.

Tauri invoke has no AbortSignal contract here. Query cancellation prevents late
cache adoption; it does not interrupt native work. No native cancellation claim
is made.

## Verification

Use real QueryClient behavior and deferred bridge promises, not a query-library
mock or an assertion that merely counts cancellation calls. Add missing tests
for these distinct behaviors:

- An initial no-data read resolves after an edit's fresh read: it cannot replace
  the latest tracks or release a claim.
- Overlapping refreshes cannot accidentally reuse one obsolete initial promise.
- A failed current read retains held controls and the last valid list; obsolete
  failures do not surface as current errors. NoSession succeeds with `[]`.
- An identical-data successful read still confirms an eligible claim.
- A newer unsent input stays held; multiple commits preserve last-sent ordering.
- A reset while a read is held rejects the old snapshot, including identical
  head/track IDs across projects.
- App mounts and provider clients are isolated; a held read resolving after
  final-observer removal cannot repopulate the cancelled cache. StrictMode
  cleanup/replay does not adopt an old result, confirm an old claim, leak
  listeners, or prevent the second boot read.
- A claim cannot be passed to a hook without its owner (typecheck regression).

Keep these issue-required suites unchanged: `App.laneControls.test.tsx`,
`App.sessionMoves.test.tsx`, and Chromium `lane-readback`, `undo-path`, `labels`,
`mix-evicted`, and `accept-b`. Preserve hook history/trail, OwnedControls, ordered
edits, `headIsApplied`, and `sessionQueueCoverage` assertions. Observe the DOM
through the slow-scheduler suite to catch ownership hand-back flicker.

Run desktop normal/slow tests, both typechecks, build, the complete Chromium
suite, Rust fmt, all-target workspace Clippy with warnings denied, and the full
Rust workspace. Generate/check third-party notices and applicable documentation
gates. No test, threshold, or scanner exclusion is weakened to make this pass.

## Handoff and risks

The query snapshot/adoption boundary and exact-version cancellation behavior are
the main risks; green unit tests alone do not establish visible correctness.
The partial migration deliberately retains ownership/queue mechanisms until
their accepted later stages. Do not claim the final deep session module or
complete #713 in this PR. Existing open drafts and unrelated research/worktrees
remain untouched; the owner reviews and merges the eventual draft.

Primary sources: [issue 713](https://github.com/laadtushar/edytlab/issues/713),
[npm metadata](https://registry.npmjs.org/@tanstack/react-query/5.104.1),
[release-pinned cancellation guide](https://github.com/TanStack/query/blob/29859ae60c8dca0a5cdbf8abccc775b655cf43e2/docs/framework/react/guides/query-cancellation.md),
[query implementation](https://github.com/TanStack/query/blob/29859ae60c8dca0a5cdbf8abccc775b655cf43e2/packages/query-core/src/query.ts),
and [query-client implementation](https://github.com/TanStack/query/blob/29859ae60c8dca0a5cdbf8abccc775b655cf43e2/packages/query-core/src/queryClient.ts).
