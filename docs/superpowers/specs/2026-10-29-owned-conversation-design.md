# Owned conversation: ADR 0007 first stage

Issue: #718. Base: 60ca7a17074dc624392c4c747ad67b9143468ec6.
Authority: accepted ADR 0007 and the user's October 29 approval of the owned
module direction, including explicit pending-step completion and tests for
interrupted approval waits before changing cancellation behavior.

## Scope

Move conversation ownership into a private `ai::conversation` module. Agent
keeps its public `conversation() -> &[Message]`, reset and withdrawal interfaces.
The module hides its Vec and exposes semantic operations, never mutable slices,
`DerefMut`, a generic push method or a raw-vector escape hatch.

Keep ModelExchange, scripted/HTTP adapters, codecs, request-size limits,
prompt snapshots, budget/summary scheduling, dispatch and approval-card policy
outside this PR. No dependency, outbound host, native command or UI change.
The PR uses Refs #718; ADR 0007 remains partial.

## Ownership and history rules

Conversation owns user-text insertion/merging, exact-tail withdrawal, reset,
thinking invalidation, assistant-message storage and ordered tool-step answers.
User text joins a trailing user message and preserves the existing two-newline
separator. Withdrawal removes only the matching final text or joined suffix,
retaining earlier text/results and removing a now-empty message.

At the existing turn-start location, remove both thinking kinds before adding
user text and selecting slim tools. Preserve explicit invalidation during a
provider's adaptation ladder before changing the request prefix. Within a turn,
preserve signed blocks and existing messages unchanged. Reject thinking-only
assistant storage; stream decoding and normalization stay in agent_loop.

An assistant step containing tool calls has explicit pending completion. Its
answers preserve call order and keep actual completed results before errors or
not-run results. Revision guidance follows answers in that same user message.
Keep existing strings and event ordering for normal success, decline, revise,
timeout, budget refusal and validation failures. No tool that ran may be recorded
as not run. Budget counting and the one-summary-request rule stay in the loop.

## Interrupted waits

Before source changes, retain an Agent, poll its turn until the held approval
card is emitted, drop the future, then inspect history and run another turn.
Prove any missing tool-result failure at runtime; no missing-symbol RED counts.
If the suspected gap is reproduced, complete the pending held calls with error
results saying the turn was interrupted before those calls ran. Preserve the
assistant message and preceding history. This is history cleanup, not native
tool cancellation or a new UI Cancel feature. No callback or network activity
is allowed from Drop. Existing PlanGate cleanup remains responsible for its slot.

A scoped pending-step guard is appropriate when it can own ordered completion
without leaking transport/policy into the module. A fully completed step must
not get duplicate Drop answers. Explicit outcomes retain prior completed results.
Arm interrupted-before-execution cleanup only while awaiting held approval:
dispatch can return before event callbacks and result storage, so a missing
recorded result is not proof that an edit never ran. The held question's Plan
callback runs inside this pre-dispatch scope; its unwinding still describes
unexecuted calls truthfully. Dispatch and result callbacks are outside cleanup.
A broader guard would need explicit execution state and separate evidence;
that recovery is outside this stage.

## Verification and compatibility

Direct plain-message tests exercise the module's real operations with independent
expected message shapes: merge/withdraw/reset, both thinking kinds, nonempty
assistant storage, ordered success/failure/not-run answers, guidance and pending
completion. Public Agent tests prove interrupted approval recovery and another
request after failure, including unknown-tool completion where coverage is absent.

Preserve existing public regression tests for orphan calls, thinking, gate,
budget, unavailable models and retry; do not replace them with private assertions.
Keep request_size.rs, prompt_snapshot.rs and provider serialization source
unchanged, apart from rustdoc links to moved history helpers. No assertions or
quality gates weakened. No live signed-thinking result is claimed without the
explicit credential-dependent test.

Always run Rust fmt, all-target workspace Clippy with warnings denied, and the
full Rust workspace. Run desktop normal/slow suites and both typechecks; build
and full Chromium provide the existing app integration gate. Website source is
unchanged. Update CLAUDE's thinking ownership rule, affected rustdoc references
and ADR index's partial Built status; accepted ADR text stays unchanged.
