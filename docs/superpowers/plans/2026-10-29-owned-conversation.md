# Owned Conversation Implementation Plan

> For agentic workers: use subagent-driven-development for the two tasks and
> independent review. The user has authorized parallel agents and this design.

**Goal:** Centralize history invariants behind an owned conversation module.
**Architecture:** Agent holds a private Conversation with a read-only message
view. Semantic operations and a pending-step lifecycle own history correctness;
transport, dispatch, approval events and budget policy keep their current roles.
**Tech Stack:** Rust 1.90, existing Tokio/serde and real Agent wiremock fixtures.
**Spec:** ../specs/2026-10-29-owned-conversation-design.md

## Global constraints

- No new dependencies, hosts, provider codecs, tool changes or frontend changes.
- Preserve Agent::conversation() -> &[Message], reset and withdrawal behavior.
- No mutable history escape hatch or generic push API.
- Keep request_size.rs and prompt_snapshot.rs unchanged and passing.
- Prove interrupted held-step history behavior before changing cancellation.
- Preserve signed history within a stable prefix and invalidate on adaptation.
- Preserve normal observable event/result order and budget scheduling.
- No weakened tests, changed scanner thresholds or new ignored cases.
- Separate worktree; one draft Refs #718 PR; never merge or mark ready.
- Conventional commits with Codex noreply author/committer and Co-Authored-By.

### Task 1: Owned history and Agent integration

Files: create crates/ai/src/conversation.rs; modify lib.rs, agent_loop.rs,
approval.rs; add public regressions in plan_first_gate.rs and, if needed,
no_orphan_tool_use.rs/model_unavailable.rs/failed_turn_leaves_no_copy.rs.
Repair moved-helper rustdoc references in responses.rs/chatgpt/provider.rs and
an existing responses_request test comment only if needed. No transport edits.

Interface consumed: current ContentBlock/Message/Role and public Agent/PlanGate.
Interface produced: private owned Conversation and semantic history operations;
public Agent interface stays unchanged. Private method spelling may follow the
clearest implementation, but mutable history must not escape the module.

- [ ] Run baseline focused public Agent targets in the verified native container.
- [ ] Add retained-Agent interrupted approval regression to the existing fixture:

```rust
let mut fx = Fixture::new(true, None, without_a_plan(vec![edit_track("t1", 0)])).await;
// Drive Agent::turn until its Plan event signals a oneshot, then drop the future.
// Assert gate closed, no edits ran, and the final user message answers t1.
// Supply subsequent classifier/model replies and assert the next request works
// with a paired tool result before its new user text.
```

  Use a future scoped in a block and tokio::select! over its real Plan event,
  not sleeps or aborting an Agent-owning task. Preserve the Agent for inspection.
- [ ] Run this test against unchanged production and record the exact runtime
  assertion failure. Send evidence to the parent before implementation.
- [ ] Implement Conversation operations and integrate all production mutations.
  Use a pending-step guard if the RED proves cleanup is needed. Preserve assistant
  history and answer only pending unexecuted calls on interrupted approval Drop.
  Drop emits no callbacks; normal paths emit events in their existing order.
- [ ] Move meaningful helper unit tests to the module and add direct lifecycle
  cases. Keep public integration assertions. Add missing unknown-tool / unavailable
  retry behavior coverage only where current tests do not prove it.
- [ ] Run focused AI suites, request-size and prompt snapshots. Run ai library
  tests and relevant ChatGPT/Responses adaptation/reasoning tests; inspect output.
- [ ] Format, self-review every history mutation and exit, scan diff for secrets,
  and commit the owned source/tests. Report commands/results and unresolved risks.

### Task 2: Staleness and final acceptance

Files: CLAUDE.md's thinking rule and docs/adr/README.md's ADR0007 Built cell.
Inputs: final Conversation method names/semantics and Task1 review.

- [ ] Replace old strip_thinking owner references with the new module and preserve
  both turn-start/adaptation invalidation rules. Describe cancellation precisely.
- [ ] Mark ADR0007 partial: conversation stage implemented; port/codecs/policy
  stages remain. Keep accepted ADR body and unrelated index rows unchanged.
- [ ] Search moved-helper links and claims, run diff-check and pinned Mint checks.
- [ ] Independent whole-branch review of the actual complete multi-commit delta.
- [ ] Run final fmt, all-target workspace Clippy -D warnings and full workspace
  tests against checksum-verified tracked files. Run desktop normal/slow, both
  typechecks/build and full Chromium once; no repeated baseline broad gates.
- [ ] Verify source unchanged since successful gates; record exact counts,
  credential-dependent limits, independent review and staleness in draft PR.

Native tests use /workspace/edytlab-native-verify.sh with one Cargo owner at a
time and persistent logs under /workspace/edytlab-validation-logs/718. New files
must be staged before syncing. Do not change the helper while it runs.
