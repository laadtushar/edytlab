---
title: "0005. Provider settings are saved in one call by one module"
sidebarTitle: "0005 Provider settings"
---

Status: Accepted, 2026-10-22.

## Context

What the agent is built from (provider, key, model, base URL, effort, profile overrides) lives in
six `AppState` slots, the keychain and the profile library. About fifteen commands each remember
three rules: wait for the startup keychain read, lift a startup refusal after a write that went
through, and rebuild the agent with the right one of two functions. Settings' Save sends up to five
commands in an order its comments explain, and merges five `{ persistent, reason }` answers. Bugs:
#394, #477, #497, #502, #504, #509, #527, #539, #569/#597.

## Decision

A desktop module, `agent_settings`, owns the keychain slots, their in-memory copy, the startup read
and the refused-at-startup state, and is the only producer of the agent's configuration.

- One command, `save_provider_settings({ provider, key?, model?, base_url?, effort? })`: the module
  waits for startup, writes in the order it needs, rebuilds the agent once, and answers once with
  `{ persistent, reason }` (a unit of work: one call, one result).
- Reads go through it and wait inside it until startup has settled.
- The keychain sits behind the existing `KeychainWrites` trait; tests use an in-memory adapter.
- The old per-field commands stay until the window has moved to the new one, then go (command
  reference, site API page and `e2e/backend.ts` change in the same PR, per CLAUDE.md).
- Kept: every write answers `{ persistent, reason }`; `has_api_key` rejects when the keychain cannot
  be read (#394, #539).

## How we will know

Tests against the in-memory keychain, no Tauri: a save during a turn is applied after it; a startup
the keychain refused builds no agent until a save lifts it; a save that fell back to session-only
says so; one save makes one rebuild. On Linux, check whether one save means fewer unlock prompts
(#527).
