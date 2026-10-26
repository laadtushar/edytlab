---
title: "0002. The plan gate is one value built on a oneshot channel"
sidebarTitle: "0002 The plan gate"
---

Status: Accepted, 2026-10-22.

## Context

Plan first shows the assistant's plan and waits for exactly one answer: approve, approve with a
revision, or reject, or a timeout. Today that handshake is four shared values (`plan_notify`,
`plan_steps_override`, `plan_rejected`, `plan_gate_open`) split between `crates/ai` (`approval::ask`,
three `Agent` fields, three `run_turn` parameters) and the desktop (`send_message` watches events to
learn whether the gate is open and resets it after the turn, because a timeout sends no event;
`approve_plan`/`reject_plan` claim it with a swap). The CLI builds the same values to pass dummies.
#251/#290 (a stray Run click banked a permit for the next turn) came from that split.

## Decision

One type in `crates/ai`, `PlanGate`, owns the protocol, built on `tokio::sync::oneshot`:

- `PlanGate::ask(plan) -> Waiting` creates a oneshot pair for this plan only; the sender sits in the
  gate's slot, the turn awaits the receiver with the timeout.
- `answer(Answer)` takes the sender out of the slot (`Option::take`) and sends. A oneshot sends one
  message; a second answer finds the slot empty and does nothing, and an answer with no question
  open does nothing, which is #251 by construction rather than by a swap on the desktop side.
- A timeout or a cancelled turn drops the receiver; the gate clears its slot, so a late answer is a
  no-op. `is_waiting()` replaces the event-watching closure.
- `Agent`, `run_turn`, `AppState` and the CLI each hold one `PlanGate` handle instead of three or
  four values. Answering never takes the agent's lock (CLAUDE.md lock rules).

## Considered and rejected

- **A struct bundling the existing `Notify` + flags.** `Agent::new`'s comment rightly says this only
  moves the argument list; the rules would still live in two crates.
- **A state machine crate.** The protocol has three states and one transition per answer; a oneshot
  in a slot is the whole machine.

## How we will know

Unit tests on `PlanGate` with no Tauri and no HTTP: one answer is delivered, a second is ignored, an
answer with no question is ignored, a timeout closes the gate, a late answer after a timeout is
ignored. `plan_first_gate.rs`, `plan_first_previews.rs` and the desktop gate tests pass unchanged.
`Agent::new` and `run_turn` each lose three parameters; `AppState` loses four fields.

Sources: https://docs.rs/tokio/latest/tokio/sync/oneshot/index.html ("A one-shot channel is used for
sending a single message between asynchronous tasks"; a dropped sender fails the receiver with
`RecvError`), read 2026-10-22.
