# Session Track Query Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Complete only ADR 0001 Stage 2: a real per-App TanStack track query behind useSession with unchanged observable editing behavior.

**Architecture:** App retains its existing owner and command handlers. A private hook owns track query/cache, read tickets, explicit cancellation, observer lifetimes, and layout-effect confirmation. The existing head/path/queue interface stays intact.

**Tech Stack:** React 19, TypeScript 5.9, TanStack React Query 5.104.1, Vitest, Testing Library, Playwright, Rust 1.90 validation.

**Spec:** `docs/superpowers/specs/2026-10-28-session-track-query-design.md`

## Global Constraints

- Pin `@tanstack/react-query` to **5.104.1**, as named by the ADR.
- No global singleton or implicit test-only fallback is allowed.
- Query keys, result envelopes, cache operations, read tickets, and cancellation stay private to the session module.
- Passing a claim without its owner must fail type checking.
- A failed, cancelled, or obsolete read releases nothing.
- No native cancellation claim is made.
- Keep these issue-required suites unchanged: `App.laneControls.test.tsx`, `App.sessionMoves.test.tsx`, and Chromium `lane-readback`, `undo-path`, `labels`, `mix-evicted`, and `accept-b`.
- Out of scope: mutations, `edit`/`move`, replacing `sessionQueue`, deleting regex guards, replacing `OwnedControls`, deleting `graphVersion`, other session queries, mix-staleness redesign, native/IPC changes, and any new outbound host.
- Generate the required third-party notices; do not hand-edit them.
- No test, threshold, or scanner exclusion is weakened to make this pass.
- Separate worktree `/workspace/edytlab/.claude/worktrees/codex-session-tracks`; one implementation writer at a time. Parent owns native Cargo synchronization/validation. No agent may run Cargo concurrently or change another worktree.
- Conventional noreply author/committer/footer; normal pushes; eventual draft uses `Refs #713`. Never merge or mark ready. Preserve all unrelated work/research.

## File map

| File | Responsibility |
| --- | --- |
| `apps/desktop/src/lib/sessionQueries.tsx` (new) | Per-App real client factory/provider, local IPC defaults |
| `apps/desktop/src/hooks/useSessionTracks.ts` (new) | Private snapshots, forced reads, ownership confirmation, reset/lifetime cancellation |
| `apps/desktop/src/hooks/useSession.ts` | Compose the track hook and typed overloads; reset track reads before head reset |
| `apps/desktop/src/App.tsx` | Provider wrapper, consume hook tracks/refresh; remove local read machinery only |
| `apps/desktop/src/__tests__/useSession.tracks.test.tsx` (new) | Real query/owner deferred-IPC integration regressions |
| `apps/desktop/src/__tests__/useSession.history.test.ts`, `useSession.trail.test.ts` | Real provider wrappers, retain all existing assertions |
| `apps/desktop/package.json`, `pnpm-lock.yaml` | Exact approved dependency and its resolution |
| `apps/desktop/src-tauri/licenses/THIRD_PARTY_NOTICES.txt` | Generated shipped-dependency notices |
| `CLAUDE.md` | Correct only descriptions of the track-read owner changed by Stage 2 |

The frontend migration is one coherent task: splitting public hook composition
from App adoption would leave a misleading intermediate migration and duplicate
test fixtures. Licensing/documentation form a separate compliance deliverable.

### Task 1: Migrate track reads and adopt the query-backed hook

**Files:** All frontend/package/lock files in the map; not CLAUDE or notices.

**Interfaces:**

- Consumes existing `OwnedControls`, `Claim`, `TrackSummary`, `listTracks`, and `isNoSession` from the bridge.
- Produces `createSessionQueryClient(): QueryClient`, `SessionQueryProvider({ children }): ReactElement`.
- Internal `useSessionTracks(owner?: OwnedControls)` produces `tracks: TrackSummary[]`, `refreshTracks(claim?: Claim): Promise<void>`, `resetTracks(): void`.
- Public `useSession()` adds tracks and no-argument refresh; `useSession({trackControls: owned})` also accepts a Claim. All existing result methods stay unchanged.

- [ ] **Step 1: Configure the approved dependency and prove a runtime RED.**

Use scoped pnpm 9.15.0 (`COREPACK_HOME=/workspace/.corepack`,
`PNPM_HOME=/workspace/.pnpm-home`, PATH prefixed by that PNPM_HOME). Use apply_patch
to add exact `"@tanstack/react-query": "5.104.1"` in desktop dependencies, then
`pnpm install` to mechanically generate the lockfile. No caret or unrelated
upgrade. Keep the real query library in tests; replace only the IPC boundary.

First create the new hook test using a fresh real QueryClientProvider and the
existing hook. Mock the bridge using the existing history/trail pattern; include
`listTracks` and real `isNoSession` recognition. Establish the current head first
so the test cannot pass before mounting. The first failing assertion is:

```tsx
const client = new QueryClient();
function Wrapper({ children }: { children: React.ReactNode }) {
  return <QueryClientProvider client={client}>{children}</QueryClientProvider>;
}
const hook = renderHook(() => useSession(), { wrapper: Wrapper });
await waitFor(() => expect(hook.result.current.head).toBe("A"));
expect(hook.result.current).toMatchObject({ tracks: [] });
```

Run `pnpm --filter @edytlab/desktop exec vitest run src/__tests__/useSession.tracks.test.tsx`.
Expected runtime assertion failure: the current hook result lacks tracks, not an
import/configuration/compilation failure. Preserve the RED log before source edits.

- [ ] **Step 2: Add provider, private query hook, and typed composition.**

The client factory sets local IPC defaults, not an application network fetcher:

```ts
new QueryClient({ defaultOptions: { queries: {
  networkMode: "always", retry: false, staleTime: Infinity,
  refetchOnWindowFocus: false, refetchOnReconnect: false,
} } });
```

`SessionQueryProvider` creates its client with a lazy React state initializer.
The private hook uses a disabled observer under `['session', 'tracks']`, a stable
empty list, and a private snapshot `{ticket, tracks}`. It does not start boot
reads or read on head changes. Implement refresh in this order:

```ts
const ticket = ++requestRef.current;
const lifetime = lifetimeRef.current;
if (claim) owner.finish(claim, ticket);
await client.cancelQueries({ queryKey: TRACKS_KEY, exact: true });
if (!current(ticket, lifetime)) return;
await client.fetchQuery({
  queryKey: TRACKS_KEY,
  staleTime: 0,
  queryFn: async () => {
    try { return { ticket, tracks: await listTracks() }; }
    catch (error) {
      if (isNoSession(error)) return { ticket, tracks: [] };
      throw error;
    }
  },
});
```

Here `current` is a private predicate requiring active lifetime, matching
lifetime generation, and latest ticket. Surround awaited reads with current-only
error propagation; an obsolete rejection returns quietly. A claim without its
owner must not be silently ignored internally either. Confirm only the matching
adopted snapshot in a layout effect using `owner?.applied(snapshot.ticket)`;
never confirm inside queryFn, after only awaiting fetch, or from an older cached
success. Ticket changes must survive structural sharing of identical arrays.

`resetTracks` advances the latest ticket before explicit exact-query cancellation.
Effect cleanup also advances observer generation and disables that lifetime;
effect setup makes reattachment active without resetting counters. Capture the
lifetime before the first await so StrictMode replay cannot revive old work.
Do not consume/forward TanStack's signal as a hidden native-abort contract.

Use overloads and named result types to enforce the public call distinction:

```ts
export function useSession(): UseSessionResult;
export function useSession(options: { trackControls: OwnedControls }):
  UseSessionWithTrackControlsResult;
```

The base result contains `refreshTracks: () => Promise<void>`; its owner-aware
counterpart exposes `(claim?: Claim) => Promise<void>`. The implementation composes
the private hook. `resetHead` invokes `resetTracks()` before the existing history
dispatch. Preserve the existing synchronous head ref, trail persistence,
compaction behavior, queue, listeners, and graphVersion.

- [ ] **Step 3: Adopt without rewriting editing behavior.**

Wrap the default exported App with `SessionQueryProvider`, and move the existing
body into an inner component. Construct existing `ownedVersion`/`OwnedControls`
before `useSession({trackControls: owned})`. Consume tracks/refresh from that
result; remove App's list state, numbered refresh, and associated bridge imports.
Keep tracksRef's passive effect, lane overlay/memo, all existing input/commit
deduplication, and every command/adoption/queue placement unchanged. Replace only
`owned.finish(claim, trackRequestRef.current + 1); await refreshTracks()` with
`await refreshTracks(claim)`. Boot remains its current single explicit refresh
mechanism with quiet errors and empty initial tracks; no auto query boot too.

Add explicit fresh real provider wrappers to hook-only history/trail fixtures;
keep their assertions. Keep required App and browser acceptance files unchanged.

- [ ] **Step 4: Add each specified read/ownership regression test-first.**

Use `held.ts` to settle promises inside act and `waitFor` to inspect the hook's
real tracks, real owner's overlay, or actual DOM. Keep full TrackSummary fixture
fields. Do not merely count calls to a mocked query client. Each scenario starts
with the relevant currently passing baseline and names its broken branch:

| Scenario | Setup/action | Observable result |
| --- | --- | --- |
| Initial no-data superseded | Hold first bridge read; complete newer refresh with gain -3; settle old gain 0 | tracks remain -3; no obsolete confirmation |
| Overlapping refreshes | Start two refreshes before initial data; settle newest first | newest independent read wins; no old-promise reuse |
| Failed confirmation | Hold gain -3, send/finish claim; reject fresh read | visible held -3 and last valid server list survive; caller sees error |
| Superseded error | Reject an older read after a later success | latest tracks unchanged; obsolete promise settles quietly |
| NoSession | Reject current bridge read with existing recognized message | successful empty tracks, no current error |
| Identical-data success | Hold/commit gain -3; confirm with server snapshot equal to prior data | claim is released after successful adoption even with shared array identity |
| New unsent input | Send -3, then set -6 before its read completes | overlay stays -6 |
| Commit ordering | Send 0, -3, 0 through unchanged owner/queue path | existing App lane suite preserves final 0 |
| Same-identity reset | Hold old read; reset same head and track ID; complete new project read then old | new project tracks and owner state cannot regress |
| Unmount/replay | Hold read, remove final observer, then resolve; remount fresh client / StrictMode boot | cancelled cache stays unpopulated; new lifetime accepts only its own read |
| Offline local IPC | Set query online manager offline in test with cleanup | explicit local read still produces tracks |

For typed safety, add a compile-only fixture in the new test file (never invoke
the function) containing `useSession().refreshTracks(claim)` with
`@ts-expect-error` and the owner-aware accepted form; typecheck must prove the
expect-error is used. Do not loosen public types or cast through the error.

- [ ] **Step 5: Verify and commit the frontend deliverable.**

Run focused new hook/history/trail/OwnedControls/App lane/App moves/structural
guard tests, then the full normal desktop suite once, both typechecks, and build.
Record exact RED/GREEN commands/results and existing canvas/chunk warnings.
Read the diff for discarded heads, premature claims, stale cache, and extra boot
reads. Run `git diff --check`, scan added lines for credential/email shapes, and
commit only owned files with noreply author/committer and required Codex footer.
No push or PR yet. Report full evidence and concerns in the task report.

### Task 2: Ship dependency notices and accurate Stage 2 documentation

**Files:** `apps/desktop/src-tauri/licenses/THIRD_PARTY_NOTICES.txt`, relevant
track-read-owner wording in `CLAUDE.md`, and the plan/spec status only.

**Interfaces:** Consumes Task 1's real bundled dependency and passing frontend;
produces generated notices verified by the repository's unchanged generator.

- [ ] **Step 1: Demonstrate stale notices using the real check.**

Coordinate with the parent's single Cargo queue. The generator needs Node,
pinned pnpm-installed dependencies, Cargo on PATH and cargo-about **0.9.2**.
Parent prepares these tools in `/workspace`, not personal home. Run
`node scripts/third-party-notices/generate.mjs --check`; expected failure reports
stale generated notices after TanStack contributes to the bundle. Preserve log;
do not accept an unrelated setup/network error as stale-output evidence.

- [ ] **Step 2: Generate, check, and document.**

Run `node scripts/third-party-notices/generate.mjs` then the identical command
with `--check`. Both must succeed. Inspect that React Query and Query Core's MIT
licenses are present and the Rust dependency inventory did not unexpectedly
change. Do not modify licensing allow-lists or hand-edit generated text.

Read writing-for-agents before changing CLAUDE. Rewrite only the lane/track-read
description made stale by Task 1; preserve the staged queue/ownership and graph
rules until Stage 3/4. For example, describe `useSession.refreshTracks` as the
owner of numbered reads and successful-snapshot hand-back, while App still owns
control input/overlay. Do not claim `edit`/`move` or deleted guards are shipped.
Run diff checks, the generator check, relevant documentation tests identified
by the repo, and commit the generated/doc changes with the same noreply footer.

## Final acceptance and handoff (controller)

- [ ] Review each task's complete multi-commit diff against its brief and
  report, with separate spec/quality verdicts; fix failures through its writer.
- [ ] Freeze frontend source before expensive validation. Run the full normal
  and slow-scheduler desktop suites, both typechecks, build, and full Chromium
  `pnpm --filter @edytlab/desktop test:e2e -- --retries=0 --workers=2`.
- [ ] Prove required App/browser files are unchanged against the base. Use
  failure-directed temporary mutations if a cancellation assertion could pass
  without the intended branch; restore all source before final gates.
- [ ] Rust formatting, workspace/all-target Clippy `-D warnings`, and full
  normal-parallel workspace tests; one native Cargo at a time using checksum
  synchronization. Prior native snapshot ref is `fb9bc356` (archive its new
  cancellation test absent from this branch). Skip no tests.
- [ ] Independent final whole-branch review, resolve findings, scan the final
  added diff for keys/emails and ensure only intended files. Preserve ledger
  rulings with their rework costs; do not silently discard findings.
- [ ] Normal push, draft PR with `Refs #713`, exact gate evidence and remaining
  Stage 3/4 scope. Attach PR, post issue handoff and update tracking #366 without
  overwriting owner changes. No merge or ready action.
