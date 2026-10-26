---
title: "0007. A turn is split at the model exchange, behind a port with two adapters"
sidebarTitle: "0007 A turn"
---

Status: Accepted, 2026-10-22. Starts after 0002.

## Context

`run_turn` (`crates/ai/src/agent_loop.rs`) is one function of about 860 lines with 18 parameters. It
classifies, assembles the prompt, fetches and gates the plan, strips old thinking, picks the slim
tool set, builds and posts the request, decodes SSE, assembles the message, enforces the tool budget
and the held step, dispatches tools and shapes their results. The conversation's rules (every
`tool_use` answered, no two user messages in a row, no thinking replayed under a changed prefix
(#468), one summary request once the budget is spent) are kept by hand at each exit. Wire-format
knowledge is split between `LlmProvider` (streamed steps) and `match wire_format()` in the loop
(one-shot calls). Testing one policy rule means writing SSE bytes per wire format.

## Decision

Ports and adapters ("hexagonal"):

- **The model exchange is a port**: `ModelExchange::step(request) -> Step` where a `Step` holds the
  text, tool calls, thinking and stop reason, streamed or one-shot. Two adapters from the start: the
  HTTP adapter (one codec per wire format: Anthropic, chat-completions, Responses, each owning both
  the streamed and the one-shot shape) and a scripted model for tests. Two real adapters are what
  justify a seam.
- **A conversation module** owns the history and enforces its rules through its operations
  (answer every call, merge user text, strip thinking when the prefix changes), so no exit path can
  leave a history the provider will reject.
- **The turn's policy** (plan, gate via 0002, budget, held step, dispatch) consumes steps from the
  port and writes through the conversation module.

Kept: the request is built once per turn (llama.cpp prompt reuse, Anthropic's prompt cache with the
full tool set); the #468 rule; the `request_size.rs` ratchets and the prompt snapshots.

## How we will know

Policy tests run against the scripted model with plain messages: budget, gate, decline, revise,
timeout, unanswered calls, thinking handling, with no SSE and no HTTP. Per-wire SSE tests remain only
for the codecs. A new wire format is one codec; `run_turn` loses its `too_many_arguments` allow.

Sources: https://alistair.cockburn.us/hexagonal-architecture/ ("developed and tested in isolation
from its eventual run-time devices and databases"), read 2026-10-22.
