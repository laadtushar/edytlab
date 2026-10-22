# 0006. The open project is one module that owns its locks

Status: Accepted, 2026-10-22.

## Context

The 98 Tauri commands (2026-10-22) reach into `AppState` (30 public fields, most `Arc<Mutex<_>>`) and take its locks
themselves: 66 `store_handle()` calls, 40 `lock_std(` calls. The lock order (store and engine in
separate scopes; `release_replaced_project`'s agent, registry, store, slot, project folder, window
mix) and "a command that waits on a lock runs on the blocking pool" are written only in CLAUDE.md and
checked by a call-graph scan of the source text. Opening a project is a long sequence, partly
repeated in `lib.rs` for the default project. Bugs: #534, #566/#629, #607/#635, #571/#613, #509,
#498/#505.

## Decision

A desktop module, `open_project`, is "the project the window has open". It owns the store slot, the
project folder, the engine, the clipboard and the window-mix and compare holds, and opening,
switching, Save As and releasing a replaced folder. Commands ask it for a few operations: read the
head's state, run an edit tool, move the head, render (through 0003), open, Save As. Its locks are
private fields, so the order is decided in one file and a command cannot take them in another
order; every wait inside it runs on the blocking pool.

It is built in stages, each a PR that leaves the app working: (1) open and switch (and `lib.rs` calls
the same path); (2) reads; (3) edits and head moves; (4) renders, after 0003.

## Considered and rejected

- **An actor (one task owns the store; commands send it messages).** It removes lock ordering as a
  class of bug, and "assigning the actor exclusive ownership of some resource" is the standard Rust
  answer for shared state. But tools run synchronously against the store while the agent loop holds
  it, so an actor needs the tool dispatch rewritten first. The module boundary chosen here is the
  same seam an actor would sit behind: if lock bugs continue after stage 3, a later record replaces
  the private locks with an actor without changing the commands.
- **Splitting `commands.rs` into files per concern.** Moves code without deepening anything; it
  follows from this and 0005.

## How we will know

Module tests without Tauri `State`: open, replace and reread; a fader click during a long tool waits
off the worker; opening A, B, A during a turn shares one store. The call-graph scan shrinks to
"commands use only this module". The #566 `LiveStore` type gate stays inside the module.

Sources: https://ryhl.io/blog/actors-with-tokio/ (2021-02-13).
