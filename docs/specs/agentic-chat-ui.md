# Agentic Chat UI

Status: proposal accompanying PR for `claude/fix-audio-wav-parsing-odiH6`.
Scope: chat panel only (`apps/desktop/src/components/Chat.tsx` and friends)
plus the minimum backend plumbing needed to support it.

## Motivation

The current assistant turn produces a wall of markdown bullets:

```
The file is loaded as track 0 with a +6 dB gain applied. You can now:

- **Preview** the current result (`render_preview`).
- **Render** a final WAV file (`render_final`).
- Apply more gain (`gain` or `set_track_gain`).
- ...
```

The user reads it as twenty undifferentiated suggestions, has to find the
relevant ones, and then has to retype them as prose for the agent. We want
the suggestions to be one-click actions, the message body to read like
chat, and the waiting state to communicate that the agent is actually
working — not frozen.

## Research summary

The patterns below come from the systems users compare us to:

- **Cursor 3**: tool calls and plans are first-class objects in the
  transcript; the agent panel has its own diff/run view alongside the
  conversation. Tools and plans are shareable artifacts, not just text
  ([Cursor 3 changelog](https://cursor.com/changelog/3-0)).
- **Claude Desktop**: connectors live behind the "+" in the composer;
  each connector advertises its tools, capabilities, and read/write
  scopes. Custom MCP servers are added the same way native connectors
  are ([Claude Help Center: connectors](https://support.claude.com/en/articles/11176164),
  [Anthropic engineering: desktop extensions](https://www.anthropic.com/engineering/desktop-extensions)).
- **Claude API extended thinking**: streams `thinking_delta` blocks that
  arrive before the natural-language reply. Best-practice in production
  apps is *not* to dump 10k thinking tokens into the chat; collapse the
  reasoning into a one-line "progress pulse" with an expand affordance
  ([Anthropic: building with extended thinking](https://platform.claude.com/docs/en/build-with-claude/extended-thinking),
  [Anthropic: streaming messages](https://platform.claude.com/docs/en/build-with-claude/streaming)).
- **LangGraph / Agent Chat UI**: nodes do one thing each; the UI
  subscribes to a stream of typed events (`on_chat_model_stream`,
  `on_tool_start`, `on_tool_end`) and renders progress at each step
  rather than waiting for the final answer
  ([LangChain Forum: agent-chat-ui streaming](https://forum.langchain.com/t/agent-chat-ui-stream-reasoning-and-tool-calls/2522)).
- **NVIDIA / MCP-orchestrator pattern**: a top-level "planner" agent
  delegates to specialized sub-agents that each own a tool set; UI
  surfaces show which sub-agent / capability is currently active so the
  user can route their next instruction
  ([NVIDIA: agentic complexity blog](https://developer.nvidia.com/blog/building-for-the-rising-complexity-of-agentic-systems-with-extreme-co-design/),
  [NVIDIA: AI-Q blueprint](https://build.nvidia.com/nvidia/aiq)).

Cross-cutting takeaway: **the agent's capabilities should be visible,
named, and toggleable from the composer**. Reasoning/thinking should be
visible but collapsible. Tool calls should resolve to compact chips, not
verbose prose.

## What we're building

Three concrete deltas. None of them require shipping real MCP servers or
real sub-agents yet — they architect the *surface* so doing that later
is local work.

### 1. Action chips on assistant messages

After the assistant finishes a turn, we want a row of one-tap chips for
the most likely next actions. Chips are derived from the tools the agent
hinted at in its message, but they are first-class buttons — clicking
one sends the chip's `prompt` field back through `sendMessage` so the
agent runs the corresponding tool.

```
[assistant bubble: "Loaded track 0, +6 dB applied."]
[▶ Preview]  [⤓ Render final]  [≡ Normalize -1 dBFS]  [↩ Undo]
```

Implementation:

- `MessageEntry` gains an optional `chips: Chip[]` field where
  `Chip = { id, label, icon, prompt }`. Chips are populated in
  `useAgentStream` when the assistant turn finishes; v1 derivation is
  pattern-matching against the agent's message ("Preview", "Render
  final", etc.) backed by a small whitelist keyed off the tool name. v2
  can move derivation to the backend so the agent emits structured
  suggestion blocks.
- Only the *most recent* assistant message renders chips. Older turns
  render plain text so the transcript stays scannable.
- Clicking a chip calls `pushUserMessage(chip.prompt)` + `sendMessage`
  exactly as if the user typed it. No new IPC surface.

### 2. Thinking indicator

Two flavors, layered:

a. **Waiting pulse** (ships in this PR). Between the user's submit and
   the first `agent://text-delta`, render a dimmed pill at the bottom of
   the scroller — "Thinking…" with a shimmer. Disappears the moment the
   first text or tool-call event arrives.

b. **Reasoning trace** (architected, not enabled). Add an
   `agent://thinking-delta` event that the backend forwards from the
   Anthropic SSE stream's `thinking_delta` blocks. The frontend appends
   into a collapsed `<details>` block at the top of the assistant
   bubble:

   ```
   ▸ Thinking (12s)         ← click to expand
       The user wants the result 6 dB louder. The file is already loaded
       so I'll call `gain` with +6, then suggest preview/render.
   ```

   Phase 1 keeps the `<details>` collapsed by default and shows only the
   elapsed timer + a one-line summary, matching the Claude Desktop
   pattern. The `details` element is keyboard-accessible by default.

   Because providers vary (OpenAI's "reasoning" surface differs from
   Anthropic's `thinking_delta`), the event payload is a flat
   `{ text: string }`; the provider abstraction in
   `crates/ai/src/provider.rs` is responsible for normalising into that
   shape.

### 3. Capabilities menu (`+` button)

Next to the composer's send button, a `+` opens a popover listing the
agent's current capabilities, grouped by class:

```
TOOLS               (built-in audio engine ops)
☑ load              ☑ render_preview     ☑ gain
☑ normalize         ☑ trim               ...

SKILLS              (workflow recipes)         [coming soon]
AGENTS              (specialized sub-agents)   [coming soon]
MCP SERVERS         (external connectors)      [+ add server]
```

Implementation:

- New Tauri command `list_capabilities() -> Capabilities` returning the
  registered tool descriptors. The MCP / Skills / Agents categories are
  declared in the type but ship with empty arrays in v1 so the UI
  always renders the same shape.
- Toggle state lives in `localStorage` keyed by capability id. When the
  user composes a turn, the disabled tool names are *not* sent in the
  request's `tools` array. The backend gains a single `disabled_tools:
  Vec<String>` field in the `SessionContext` (already plumbed via
  `set_selection_context`-style commands) so the UI can drive it
  per-turn.
- Capabilities defaults to *all-on* on first launch — this matches the
  Cursor / Claude Desktop expectation that the assistant starts with
  everything available, and the user can subtract.

Out of scope for this PR (explicitly):

- Running real MCP servers (requires JSON-RPC client + sandbox).
- Custom user-authored skills / agents (requires a skills runtime).
- Persisting toggles across machines.

These are listed in the menu with a "Coming soon" subtitle so the
surface is in place when we build them; today the entries just inform
the user what we plan to support.

## Backend changes summary

| File | Change |
| --- | --- |
| `crates/tools/src/tool/load.rs` | **(this PR)** transcode non-WAV sources to CAS WAVs so the M22 renderer's WAV-only contract holds. Fixes "Ill-formed WAVE file: no RIFF tag found". |
| `apps/desktop/src-tauri/src/commands.rs` | New `list_capabilities` command. Surfaces tool descriptors from the dispatcher; MCP/Skills/Agents groups are empty placeholders. |
| `apps/desktop/src-tauri/src/events.rs` | Emit `agent://tool-call-end { id, ok }` (already produced by the agent loop, just not forwarded). Stub `agent://thinking-delta { text }` for phase 2. |
| `crates/ai/src/provider.rs` | (phase 2) parse `thinking_delta` blocks per provider and emit `AgentEvent::ThinkingDelta(String)`. Disabled-by-default in this PR — the event is reserved. |

## Frontend changes summary

| File | Change |
| --- | --- |
| `apps/desktop/src/components/MessageBubble.tsx` | Optional `chips` slot, rendered as a row of pill buttons beneath the bubble. |
| `apps/desktop/src/components/ThinkingIndicator.tsx` | New. Pulse pill while awaiting first delta. |
| `apps/desktop/src/components/CapabilitiesMenu.tsx` | New. Popover triggered by `+` in the composer. |
| `apps/desktop/src/hooks/useAgentStream.ts` | Add `awaiting` flag (true after submit, false after first text/tool/plan event). Hook up tool-call-end to resolve badge status. Derive chips on `done`. |
| `apps/desktop/src/hooks/useCapabilities.ts` | New. Loads capabilities once, tracks toggle state in `localStorage`. |
| `apps/desktop/src/lib/tauri-bridge.ts` | `listCapabilities` invoker; `onToolCallEnd` listener. |

## Non-goals

- Re-doing the message-bubble *visual* design beyond adding the chip
  row. The current Studio Onyx asymmetric bubble already reads well.
- Multi-agent orchestration. We adopt the *vocabulary* (skills, agents,
  MCP, tools) so the surface scales when we add them, but the runtime
  stays single-agent for now.
- Streaming-token-level reasoning in the UI. We collapse it behind a
  `<details>` to match production Claude apps; full inline expansion is
  a future preference toggle.

## Acceptance criteria

- "Thinking…" pill appears within 50ms of submit and disappears on first
  delta / tool-call / plan event.
- The most recent assistant message renders ≥1 chip when the message
  mentions a known tool; clicking the chip submits the chip's prompt
  exactly as if typed.
- `+` in the composer opens a popover whose tools list matches
  `ToolDispatcher::tool_schemas()` order-stable per launch. Toggling a
  tool off filters it out of subsequent agent turns (verified by an
  e2e test against the backend's `disabled_tools` echo).
- A `load` of a non-WAV source (mp3/flac) followed by `render_preview`
  succeeds — no "Ill-formed WAVE file" error.
