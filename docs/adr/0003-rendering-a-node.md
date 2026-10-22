# 0003. One module renders a node and returns what it left out

Status: Accepted, 2026-10-22.

## Context

Every path that renders a node must (1) put back audio a sweep removed (`rederive::materialize`),
(2) use the bounded preview cache and take a hold when the window will read the file again, and
(3) report the stored effects the renderer left out (#520, #531, #542). These rules are prose in
CLAUDE.md; `materialize` has nine call sites in two crates; the warnings come in three forms
(`report.warnings` sentences, `audio_engine::render_warnings`, the desktop's `render_warnings_of`);
there are two copies of "a node's whole mix into the cache" (the `render_preview` tool and the
desktop's `cached_mix`). Bugs: #520/#531/#532/#542, #535/#563, #575/#606, #607/#635, #98/#371.

## Decision

A module in `crates/tools`, `render`, is the only way to render a node:

- `render::node(store, engine, request) -> Result<Rendered>` where the request names the node, the
  span (whole mix or a range) and the target (the preview cache, optionally held, or a file the
  caller names).
- It always puts the node's audio back first, looks up or fills the cache, takes the hold when
  asked, and returns `Rendered { path, hold: Option<Held>, left_out: Vec<LeftOutEffect> }`.
- `LeftOutEffect` is one typed list; the tools' `warnings` sentences and the desktop's
  `RenderWarning` are built from it, so the three forms become one.
- Callers: the `render_preview` and `render_final` tools, the desktop's `render_preview`,
  `prepare_compare` and `render_range` commands. `cached_mix` is deleted.
- The result type is the enforcement: a path that renders cannot get a file without the list of
  what it left out, so it cannot forget to report it ("make illegal states unrepresentable").

`Engine` (a unit struct behind `Arc<Mutex<_>>`, bypassed by two tools) is kept for now: whether the
mutex limits concurrent renders' memory has not been measured. A later record may remove it.

## Considered and rejected

- **A lint or test listing every render call site.** That is the current approach (per-tool tests)
  and it is what let new paths forget the rules.

## How we will know

The tools' JSON and the commands' answers are unchanged (the command reference and
`docs/tools-reference.md` diff empty); `read_only_tools.rs` passes; one module test covers a swept
node rendering, a cache hit reporting the same left-out effects as the render that made it, and a
held entry surviving eviction.
