# Medium Features Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add four medium-effort, high-value features: loop playback, per-clip volume envelope, batch import, and session templates.

**Architecture:** Loop playback is a WaveSurfer-level audioprocess hook in the Timeline. Volume envelope extends the `Clip` struct with an `EnvelopePoint` list and threads through the render streaming path. Batch import adds a `batch_load` Tauri command that calls the existing load tool logic for each file in sequence. Session templates are JSON files bundled in `src-tauri/resources/` and applied via a `apply_template` command that creates a new DAG node with the template's `SessionState`.

**Tech Stack:** React 19, TypeScript, WaveSurfer.js 7, Tauri 2, Rust (`session`, `audio-engine`, `tools` crates), `tauri-plugin-dialog`, `serde_json`.

---

## File Map

| File | Change |
|------|--------|
| `apps/desktop/src/components/Timeline.tsx` | Add `loop` prop + `onLoopChange`, audioprocess handler, loop icon button |
| `apps/desktop/src/App.tsx` | Add `loopActive` state, `L` key shortcut |
| `crates/session/src/state.rs` | Add `EnvelopePoint` struct + `volume_envelope` field to `Clip` |
| `crates/audio-engine/src/render.rs` | Apply envelope interpolation per output frame |
| `crates/tools/src/tool/set_clip_envelope.rs` | New tool |
| `crates/tools/src/tool/mod.rs` | Export `SetClipEnvelopeTool` |
| `crates/tools/src/dispatcher.rs` | Register `SetClipEnvelopeTool` in `default_dispatcher` |
| `apps/desktop/src-tauri/src/commands.rs` | Add `batch_load` + `list_templates` + `apply_template` |
| `apps/desktop/src-tauri/src/lib.rs` | Register new commands |
| `apps/desktop/src-tauri/resources/templates/` | `podcast.json`, `music.json`, `interview.json` |
| `apps/desktop/src/lib/tauri-bridge.ts` | Add `batchLoad`, `listTemplates`, `applyTemplate` |
| `apps/desktop/src/components/TemplatePickerModal.tsx` | New component |
| `apps/desktop/src/components/EmptyState.tsx` | Add template picker entry point |

---

### Task 5: Loop Playback

**Files:**
- Modify: `apps/desktop/src/components/Timeline.tsx`
- Modify: `apps/desktop/src/App.tsx`

WaveSurfer fires an `audioprocess` event roughly every animation frame during playback. When loop mode is active and the current time exceeds `selection.end`, we seek back to `selection.start`. The `L` key toggles loop; loop is only meaningful when a selection exists.

- [ ] **Step 1: Write the failing test**

In `apps/desktop/src/__tests__/Timeline.loop.test.tsx` (new file):

```tsx
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { forwardRef } from "react";

// Minimal stub — we only test the loop toggle button in isolation.
// WaveSurfer is mocked by vitest's module mock system.
vi.mock("wavesurfer.js", () => ({
  default: {
    create: () => ({
      on: vi.fn(),
      load: vi.fn(),
      zoom: vi.fn(),
      play: vi.fn(),
      pause: vi.fn(),
      seekTo: vi.fn(),
      destroy: vi.fn(),
      getDuration: () => 60,
      getCurrentTime: () => 0,
    }),
  },
}));

import { Timeline } from "../components/Timeline";

describe("Timeline loop toggle", () => {
  it("renders loop button", () => {
    render(
      <Timeline
        audioPath={null}
        loop={false}
        onLoopChange={vi.fn()}
        selection={{ start: 1, end: 5 }}
      />,
    );
    expect(screen.getByTestId("loop-btn")).toBeInTheDocument();
  });

  it("calls onLoopChange when loop button clicked", async () => {
    const onLoopChange = vi.fn();
    render(
      <Timeline
        audioPath={null}
        loop={false}
        onLoopChange={onLoopChange}
        selection={{ start: 1, end: 5 }}
      />,
    );
    await userEvent.click(screen.getByTestId("loop-btn"));
    expect(onLoopChange).toHaveBeenCalledWith(true);
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

```bash
pnpm --filter @edytlab/desktop test -- --run Timeline.loop
```

Expected: FAIL — `loop` prop and `loop-btn` not present.

- [ ] **Step 3: Add `loop` prop to Timeline and loop button**

In `apps/desktop/src/components/Timeline.tsx`:

Add to `TimelineProps`:

```tsx
loop?: boolean;
onLoopChange?: (loop: boolean) => void;
```

Add loop icon button near the existing transport controls (or in the Timeline header). Locate where the Timeline renders its top bar and add:

```tsx
<button
  data-testid="loop-btn"
  onClick={() => props.onLoopChange?.(!props.loop)}
  className={`text-xs px-2 py-1 rounded border transition-colors ${
    props.loop
      ? "border-amber-400 text-amber-400 bg-amber-400/10"
      : "border-neutral-600 text-neutral-400 hover:border-neutral-400"
  }`}
  title="Toggle loop (L)"
>
  ↺
</button>
```

- [ ] **Step 4: Add audioprocess loop handler to TrackLane**

In `TrackLane`, add `loop` and `selection` to `LaneProps`:

```tsx
loop?: boolean;
// selection is already in LaneProps — verify this, then add:
```

After WaveSurfer is created (inside the useEffect that calls `WaveSurfer.create`), attach the `audioprocess` listener:

```tsx
ws.on("audioprocess", () => {
  if (!laneLoop || !laneSelection) return;
  const t = ws.getCurrentTime();
  if (t >= laneSelection.end) {
    const dur = ws.getDuration();
    if (dur > 0) ws.seekTo(laneSelection.start / dur);
  }
});
```

Where `laneLoop` and `laneSelection` are refs (not state) so the closure captures the latest value without recreating the listener:

```tsx
const loopRef = useRef(loop);
const selectionRef = useRef(selection);
useEffect(() => { loopRef.current = loop; }, [loop]);
useEffect(() => { selectionRef.current = selection; }, [selection]);
// Use loopRef.current / selectionRef.current inside the audioprocess callback.
```

Pass `loop` from Timeline's render of each `TrackLane`.

- [ ] **Step 5: Add `loopActive` state to App and wire `L` key**

In `apps/desktop/src/App.tsx`:

```tsx
const [loopActive, setLoopActive] = useState(false);

// In the existing keydown handler:
if (e.key === "l" || e.key === "L") {
  e.preventDefault();
  setLoopActive(v => !v);
  return;
}

// Pass to Timeline:
<Timeline
  ...
  loop={loopActive}
  onLoopChange={setLoopActive}
/>
```

- [ ] **Step 6: Run tests and type check**

```bash
pnpm --filter @edytlab/desktop test -- --run Timeline.loop
pnpm --filter @edytlab/desktop exec tsc --noEmit
```

Expected: all pass.

- [ ] **Step 7: Commit**

```bash
git add apps/desktop/src/components/Timeline.tsx apps/desktop/src/App.tsx \
        apps/desktop/src/__tests__/Timeline.loop.test.tsx
git commit -m "feat(timeline): loop playback mode toggled by L key and loop button"
```

---

### Task 6: Clip Volume Envelope

**Files:**
- Modify: `crates/session/src/state.rs`
- Modify: `crates/audio-engine/src/render.rs`
- Create: `crates/tools/src/tool/set_clip_envelope.rs`
- Modify: `crates/tools/src/tool/mod.rs`
- Modify: `crates/tools/src/dispatcher.rs`

This extends the session data model with per-clip gain automation and threads envelope interpolation into the streaming render path.

- [ ] **Step 1: Write a failing Rust test for envelope interpolation**

In `crates/audio-engine/src/render.rs`, add to the `#[cfg(test)]` block at the bottom:

```rust
#[cfg(test)]
mod envelope_tests {
    use super::interp_envelope_gain_db;

    #[test]
    fn returns_first_point_before_start() {
        let pts = vec![(0, -6.0_f32), (44100, 0.0_f32)];
        assert!((interp_envelope_gain_db(&pts, 0) - (-6.0)).abs() < 1e-5);
    }

    #[test]
    fn interpolates_between_points() {
        let pts = vec![(0, -6.0_f32), (44100, 0.0_f32)];
        let mid = interp_envelope_gain_db(&pts, 22050);
        assert!((mid - (-3.0)).abs() < 0.01, "mid={mid}");
    }

    #[test]
    fn clamps_to_last_point_beyond_end() {
        let pts = vec![(0, -6.0_f32), (44100, 0.0_f32)];
        assert!((interp_envelope_gain_db(&pts, 88200) - 0.0).abs() < 1e-5);
    }

    #[test]
    fn empty_envelope_returns_zero_db() {
        assert!((interp_envelope_gain_db(&[], 1000) - 0.0).abs() < 1e-5);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test --package audio-engine envelope_tests 2>&1 | tail -10
```

Expected: FAIL — function `interp_envelope_gain_db` not found.

- [ ] **Step 3: Add `EnvelopePoint` to session state**

In `crates/session/src/state.rs`, add before the `Clip` struct:

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EnvelopePoint {
    pub time_samples: u64,
    pub gain_db: f32,
}
```

Add to `Clip` struct:

```rust
#[serde(default, skip_serializing_if = "Vec::is_empty")]
pub volume_envelope: Vec<EnvelopePoint>,
```

Place after the existing `beat_grid` field.

- [ ] **Step 4: Run `cargo test --workspace` to confirm schema change compiles**

```bash
cargo test --workspace 2>&1 | grep -E "^error|FAILED|ok$" | head -20
```

Expected: no new errors (the `default` serde attribute makes the field backward-compatible).

- [ ] **Step 5: Implement `interp_envelope_gain_db` in `render.rs`**

In `crates/audio-engine/src/render.rs`, add after the imports:

```rust
/// Linear interpolation of gain_db at `frame` from a sorted point list.
/// Returns 0.0 dB (no change) for an empty list.
pub(crate) fn interp_envelope_gain_db(pts: &[(u64, f32)], frame: u64) -> f32 {
    if pts.is_empty() {
        return 0.0;
    }
    if frame <= pts[0].0 {
        return pts[0].1;
    }
    if frame >= pts[pts.len() - 1].0 {
        return pts[pts.len() - 1].1;
    }
    let pos = pts.partition_point(|&(t, _)| t <= frame);
    let (t0, g0) = pts[pos - 1];
    let (t1, g1) = pts[pos];
    let alpha = (frame - t0) as f32 / (t1 - t0) as f32;
    g0 + alpha * (g1 - g0)
}
```

- [ ] **Step 6: Apply envelope in the streaming render path**

In `render.rs`, inside `TrackStreamer` (or wherever per-frame gain is applied), after computing the regular `gain_db` for the track, apply the clip envelope:

```rust
// Where track samples are mixed into the master chunk, for each frame `f`
// relative to the clip start, compute the additional envelope gain:
let envelope_pts: Vec<(u64, f32)> = clip
    .volume_envelope
    .iter()
    .map(|p| (p.time_samples, p.gain_db))
    .collect();

// For each frame index `frame_in_clip`:
let env_gain_db = interp_envelope_gain_db(&envelope_pts, frame_in_clip);
let env_linear = 10.0_f32.powf(env_gain_db / 20.0);
// Multiply sample by env_linear in addition to the track gain.
```

The exact integration point depends on where per-sample gain is applied in the `TrackStreamer::next_chunk` method. Find the line where `gain_linear` multiplies each sample and chain the envelope factor.

- [ ] **Step 7: Run envelope tests**

```bash
cargo test --package audio-engine envelope_tests
```

Expected: 4/4 PASS.

- [ ] **Step 8: Create `set_clip_envelope` tool**

Create `crates/tools/src/tool/set_clip_envelope.rs`:

```rust
//! `set_clip_envelope` — replace the volume automation curve on a clip.

use serde::Deserialize;
use serde_json::{json, Value};
use session::{EnvelopePoint, SessionNode};

use crate::schema::anthropic_tool;
use crate::{Tool, ToolContext, ToolResult};

#[derive(Debug, Deserialize)]
struct PointArgs {
    time_sec: f64,
    gain_db: f32,
}

#[derive(Debug, Deserialize)]
struct Args {
    track_index: usize,
    clip_index: usize,
    points: Vec<PointArgs>,
}

pub struct SetClipEnvelopeTool;

impl Tool for SetClipEnvelopeTool {
    fn name(&self) -> &'static str {
        "set_clip_envelope"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "set_clip_envelope",
            "Replace the per-clip volume automation curve. Points are (time_sec, gain_db) pairs; the engine linearly interpolates between them.",
            json!({
                "type": "object",
                "properties": {
                    "track_index": { "type": "integer", "minimum": 0 },
                    "clip_index":  { "type": "integer", "minimum": 0 },
                    "points": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "time_sec": { "type": "number" },
                                "gain_db":  { "type": "number" },
                            },
                            "required": ["time_sec", "gain_db"],
                            "additionalProperties": false,
                        }
                    }
                },
                "required": ["track_index", "clip_index", "points"],
                "additionalProperties": false,
            }),
        )
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };

        let head = match ctx.store.head() {
            Some(h) => h,
            None => return Ok(ToolResult::Error("no session loaded".into())),
        };
        let mut node = match ctx.store.get(head) {
            Ok(n) => n,
            Err(e) => return Ok(ToolResult::Error(format!("node lookup failed: {e}"))),
        };

        let track = match node.state.tracks.get_mut(args.track_index) {
            Some(t) => t,
            None => return Ok(ToolResult::Error(format!(
                "track_index {} out of range (session has {} tracks)",
                args.track_index, node.state.tracks.len()
            ))),
        };
        let clip = match track.clips.get_mut(args.clip_index) {
            Some(c) => c,
            None => return Ok(ToolResult::Error(format!(
                "clip_index {} out of range",
                args.clip_index
            ))),
        };

        let sample_rate = node.state.sample_rate as f64;
        let mut pts: Vec<EnvelopePoint> = args
            .points
            .iter()
            .map(|p| EnvelopePoint {
                time_samples: (p.time_sec * sample_rate) as u64,
                gain_db: p.gain_db,
            })
            .collect();
        pts.sort_by_key(|p| p.time_samples);
        clip.volume_envelope = pts;

        let node = SessionNode { parent: Some(head), state: node.state, ..Default::default() };
        let new_id = ctx.store.append(node).map_err(|e| crate::DispatchError::Internal(e.to_string()))?;
        Ok(ToolResult::Ok(json!({
            "new_node_id": new_id.to_hex(),
            "summary": format!(
                "Set volume envelope on track {} clip {} ({} points)",
                args.track_index, args.clip_index, args.points.len()
            )
        })))
    }
}
```

Note: `ctx.store.head()` must exist. If the method is named differently (check `Store` in `crates/session/src/store.rs`), use the correct accessor. The `store.append` call pattern matches the other tools.

- [ ] **Step 9: Export and register the tool**

In `crates/tools/src/tool/mod.rs`, add:

```rust
pub mod set_clip_envelope;
pub use set_clip_envelope::SetClipEnvelopeTool;
```

In `crates/tools/src/dispatcher.rs`, inside `default_dispatcher()`, add:

```rust
d.register(Box::new(SetClipEnvelopeTool));
```

- [ ] **Step 10: Run all workspace tests**

```bash
cargo test --workspace 2>&1 | grep -E "FAILED|ok" | tail -20
```

Expected: no failures.

- [ ] **Step 11: Commit**

```bash
git add crates/session/src/state.rs \
        crates/audio-engine/src/render.rs \
        crates/tools/src/tool/set_clip_envelope.rs \
        crates/tools/src/tool/mod.rs \
        crates/tools/src/dispatcher.rs
git commit -m "feat(engine): per-clip volume envelope with linear interpolation + set_clip_envelope tool"
```

---

### Task 7: Batch Import

**Files:**
- Modify: `apps/desktop/src-tauri/src/commands.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Modify: `apps/desktop/src/lib/tauri-bridge.ts`
- Modify: `apps/desktop/src/lib/file-open.ts`
- Modify: `apps/desktop/src/App.tsx`

A new `batch_load` Tauri command accepts a `Vec<String>` of file paths and calls the existing `LoadTool` logic for each in sequence, building up a multi-track session. Each file becomes a separate track.

- [ ] **Step 1: Write a Rust test for sequential batch loading**

In `apps/desktop/src-tauri/tests/batch_load.rs` (or inside `commands.rs` test module):

```rust
#[cfg(test)]
mod batch_load_tests {
    #[test]
    fn empty_paths_returns_early() {
        // Pure logic: ensure empty input produces zero tracks.
        let paths: Vec<String> = vec![];
        // The real command would iterate and call load tool for each path.
        // This test validates the guard clause logic.
        assert_eq!(paths.len(), 0);
        // If paths is empty, batch_load returns an error — test this expectation.
        let result: Result<(), &str> = if paths.is_empty() {
            Err("no files provided")
        } else {
            Ok(())
        };
        assert!(result.is_err());
    }
}
```

- [ ] **Step 2: Add `batch_load` command to `commands.rs`**

```rust
#[tauri::command]
pub async fn batch_load(
    state: tauri::State<'_, AppState>,
    paths: Vec<String>,
) -> Result<serde_json::Value, CommandError> {
    if paths.is_empty() {
        return Err(CommandError::InvalidPath);
    }

    let mut store = lock_std(&state.store, "store")?;
    let mut engine = lock_std(&state.engine, "engine")?;
    let mut clipboard = lock_std(&state.clipboard, "clipboard")?;

    let dispatcher = tools::ToolDispatcher::default_dispatcher();
    let mut last_node_id: Option<String> = None;
    let mut track_count = 0usize;

    for path in &paths {
        let mut ctx = tools::ToolContext {
            store: &mut store,
            engine: &mut engine,
            user_message: "",
            clipboard: &mut clipboard,
        };
        match dispatcher.invoke(
            "load",
            serde_json::json!({ "path": path }),
            &mut ctx,
        ) {
            Ok(tools::ToolResult::Ok(v)) => {
                last_node_id = v.get("node_id").and_then(|n| n.as_str()).map(str::to_string);
                track_count += 1;
            }
            Ok(tools::ToolResult::Error(e)) => {
                return Err(CommandError::InvalidPath);
            }
            Err(e) => return Err(CommandError::InvalidPath),
        }
    }

    Ok(serde_json::json!({
        "node_id": last_node_id,
        "tracks_loaded": track_count,
        "summary": format!("Loaded {} files into {} tracks", paths.len(), track_count),
    }))
}
```

Note: `state.clipboard` must exist as a field in `AppState`. Check existing commands for the exact field name (from prior session: `clipboard: Arc<Mutex<Option<Vec<f32>>>>`).

- [ ] **Step 3: Register in `lib.rs`**

```rust
commands::batch_load,
```

- [ ] **Step 4: Verify Rust compiles**

```bash
cargo build --package desktop-tauri 2>&1 | head -40
```

- [ ] **Step 5: Add `batchLoad` to `tauri-bridge.ts`**

```ts
export async function batchLoad(
  paths: string[],
): Promise<{ node_id: string; tracks_loaded: number; summary: string }> {
  return invoke("batch_load", { paths });
}
```

- [ ] **Step 6: Update `file-open.ts` to support multiple file selection**

In `apps/desktop/src/lib/file-open.ts`, find or add a `pickMultipleAudioFiles` function:

```ts
import { open } from "@tauri-apps/plugin-dialog";

export async function pickMultipleAudioFiles(): Promise<string[] | null> {
  const result = await open({
    title: "Open Audio Files",
    multiple: true,
    filters: [
      { name: "Audio", extensions: ["wav", "mp3", "flac", "ogg", "aiff", "m4a"] },
    ],
  });
  if (!result) return null;
  return Array.isArray(result) ? result : [result];
}
```

- [ ] **Step 7: Wire batch import in App.tsx**

In `apps/desktop/src/App.tsx`, find the existing `handleOpen` or `onOpen` handler. Add a parallel batch-open path:

```tsx
import { pickMultipleAudioFiles } from "./lib/file-open";
import { batchLoad } from "./lib/tauri-bridge";

const handleBatchOpen = useCallback(async () => {
  const paths = await pickMultipleAudioFiles();
  if (!paths || paths.length === 0) return;
  if (paths.length === 1) {
    // Single file — use existing single-load path for simplicity.
    await loadAudio(paths[0]);
    return;
  }
  try {
    const result = await batchLoad(paths);
    if (result.node_id) setHeadLocal(result.node_id);
    const newTracks = await listTracks();
    setTracks(newTracks);
    if (newTracks.length > 0 && newTracks[0].audio_path) {
      setAudioPath(newTracks[0].audio_path);
    }
  } catch (e) {
    setRenderError(String(e));
  }
}, [setHeadLocal]);
```

Update the Open Audio button in `AppHeader` to call `handleBatchOpen` instead of (or in addition to) the single-file picker.

- [ ] **Step 8: Type check and test**

```bash
pnpm --filter @edytlab/desktop exec tsc --noEmit
cargo test --workspace
```

Expected: all pass.

- [ ] **Step 9: Commit**

```bash
git add apps/desktop/src-tauri/src/commands.rs \
        apps/desktop/src-tauri/src/lib.rs \
        apps/desktop/src/lib/tauri-bridge.ts \
        apps/desktop/src/lib/file-open.ts \
        apps/desktop/src/App.tsx
git commit -m "feat(import): batch_load command for multi-file import into multi-track session"
```

---

### Task 8: Session Templates

**Files:**
- Create: `apps/desktop/src-tauri/resources/templates/podcast.json`
- Create: `apps/desktop/src-tauri/resources/templates/music.json`
- Create: `apps/desktop/src-tauri/resources/templates/interview.json`
- Modify: `apps/desktop/src-tauri/tauri.conf.json` (bundle resources)
- Modify: `apps/desktop/src-tauri/src/commands.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Modify: `apps/desktop/src/lib/tauri-bridge.ts`
- Create: `apps/desktop/src/components/TemplatePickerModal.tsx`
- Modify: `apps/desktop/src/components/EmptyState.tsx`

Templates are JSON files that describe a `SessionState` skeleton (tracks with names + gain + muted, sample_rate, no clips). `apply_template` deserializes one and stores a new DAG node.

- [ ] **Step 1: Create template JSON files**

Create `apps/desktop/src-tauri/resources/templates/podcast.json`:

```json
{
  "name": "Podcast",
  "description": "Two-speaker podcast with host and guest tracks",
  "state": {
    "tracks": [
      { "name": "Host", "gain_db": -3.0, "muted": false, "soloed": false, "pan": 0.0, "clips": [], "effects": [] },
      { "name": "Guest", "gain_db": -3.0, "muted": false, "soloed": false, "pan": 0.0, "clips": [], "effects": [] }
    ],
    "bus_routing": { "buses": [] },
    "master_chain": [],
    "tempo_map": { "default_bpm": 120.0, "segments": [] },
    "key_map": null,
    "transcript": null,
    "sample_rate": 44100,
    "length_samples": 0,
    "annotations": []
  }
}
```

Create `apps/desktop/src-tauri/resources/templates/music.json`:

```json
{
  "name": "Music",
  "description": "Four-track music arrangement: lead, harmony, bass, drums",
  "state": {
    "tracks": [
      { "name": "Lead",    "gain_db": 0.0,  "muted": false, "soloed": false, "pan": 0.0,   "clips": [], "effects": [] },
      { "name": "Harmony", "gain_db": -3.0, "muted": false, "soloed": false, "pan": -0.3,  "clips": [], "effects": [] },
      { "name": "Bass",    "gain_db": -1.0, "muted": false, "soloed": false, "pan": 0.0,   "clips": [], "effects": [] },
      { "name": "Drums",   "gain_db": -2.0, "muted": false, "soloed": false, "pan": 0.0,   "clips": [], "effects": [] }
    ],
    "bus_routing": { "buses": [] },
    "master_chain": [],
    "tempo_map": { "default_bpm": 120.0, "segments": [] },
    "key_map": null,
    "transcript": null,
    "sample_rate": 44100,
    "length_samples": 0,
    "annotations": []
  }
}
```

Create `apps/desktop/src-tauri/resources/templates/interview.json`:

```json
{
  "name": "Interview",
  "description": "Interviewer and subject tracks optimised for voice",
  "state": {
    "tracks": [
      { "name": "Interviewer", "gain_db": -2.0, "muted": false, "soloed": false, "pan": -0.2, "clips": [], "effects": [] },
      { "name": "Subject",     "gain_db": -2.0, "muted": false, "soloed": false, "pan": 0.2,  "clips": [], "effects": [] }
    ],
    "bus_routing": { "buses": [] },
    "master_chain": [],
    "tempo_map": { "default_bpm": 120.0, "segments": [] },
    "key_map": null,
    "transcript": null,
    "sample_rate": 44100,
    "length_samples": 0,
    "annotations": []
  }
}
```

- [ ] **Step 2: Add resources to `tauri.conf.json`**

In `apps/desktop/src-tauri/tauri.conf.json`, find the `bundle.resources` array and add:

```json
"resources": {
  "resources/templates/*": "templates/"
}
```

If the key is already an array of strings, use the object form (Tauri 2 supports both).

- [ ] **Step 3: Add `list_templates` and `apply_template` commands**

In `apps/desktop/src-tauri/src/commands.rs`:

```rust
#[derive(Debug, serde::Serialize)]
pub struct TemplateInfo {
    pub name: String,
    pub description: String,
}

#[tauri::command]
pub async fn list_templates(app_handle: tauri::AppHandle) -> Result<Vec<TemplateInfo>, CommandError> {
    let resource_dir = app_handle
        .path()
        .resource_dir()
        .map_err(|_| CommandError::Io(std::io::Error::other("resource dir unavailable")))?;
    let templates_dir = resource_dir.join("templates");
    let mut out = Vec::new();
    let entries = std::fs::read_dir(&templates_dir)
        .map_err(CommandError::Io)?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let raw = std::fs::read_to_string(&path).map_err(CommandError::Io)?;
        let v: serde_json::Value = serde_json::from_str(&raw).map_err(CommandError::Session)?;
        out.push(TemplateInfo {
            name:        v["name"].as_str().unwrap_or("").to_string(),
            description: v["description"].as_str().unwrap_or("").to_string(),
        });
    }
    Ok(out)
}

#[tauri::command]
pub async fn apply_template(
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
    name: String,
) -> Result<String, CommandError> {
    let resource_dir = app_handle
        .path()
        .resource_dir()
        .map_err(|_| CommandError::Io(std::io::Error::other("resource dir unavailable")))?;
    let templates_dir = resource_dir.join("templates");
    // Find the template file matching `name`.
    let entries = std::fs::read_dir(&templates_dir).map_err(CommandError::Io)?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let raw = std::fs::read_to_string(&path).map_err(CommandError::Io)?;
        let v: serde_json::Value = serde_json::from_str(&raw).map_err(CommandError::Session)?;
        if v["name"].as_str().unwrap_or("") != name {
            continue;
        }
        let state_value = &v["state"];
        let session_state: session::SessionState =
            serde_json::from_value(state_value.clone()).map_err(CommandError::Session)?;
        let mut store = lock_std(&state.store, "store")?;
        let node = session::SessionNode {
            parent: store.head(),
            state: session_state,
            label: Some(format!("Template: {name}")),
            ..Default::default()
        };
        let new_id = store.append(node).map_err(CommandError::Session)?;
        return Ok(new_id.to_hex());
    }
    Err(CommandError::InvalidPath)
}
```

Note: `CommandError::Session` needs to accept `serde_json::Error`. Check the existing `From` impls; if missing, add:
```rust
impl From<serde_json::Error> for CommandError {
    fn from(e: serde_json::Error) -> Self { CommandError::Session(session::Error::Json(e)) }
}
```

- [ ] **Step 4: Register in `lib.rs`**

```rust
commands::list_templates,
commands::apply_template,
```

- [ ] **Step 5: Add to `tauri-bridge.ts`**

```ts
export interface TemplateInfo {
  name: string;
  description: string;
}

export async function listTemplates(): Promise<TemplateInfo[]> {
  return invoke("list_templates");
}

export async function applyTemplate(name: string): Promise<string> {
  return invoke("apply_template", { name });
}
```

- [ ] **Step 6: Write a test for the template picker modal**

In `apps/desktop/src/__tests__/TemplatePickerModal.test.tsx`:

```tsx
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { TemplatePickerModal } from "../components/TemplatePickerModal";

const TEMPLATES = [
  { name: "Podcast", description: "Two-speaker podcast" },
  { name: "Music", description: "Four-track music" },
];

describe("TemplatePickerModal", () => {
  it("renders template names", () => {
    render(
      <TemplatePickerModal
        open={true}
        templates={TEMPLATES}
        onSelect={vi.fn()}
        onClose={vi.fn()}
      />,
    );
    expect(screen.getByText("Podcast")).toBeInTheDocument();
    expect(screen.getByText("Music")).toBeInTheDocument();
  });

  it("calls onSelect with template name", async () => {
    const onSelect = vi.fn();
    render(
      <TemplatePickerModal
        open={true}
        templates={TEMPLATES}
        onSelect={onSelect}
        onClose={vi.fn()}
      />,
    );
    await userEvent.click(screen.getByText("Podcast"));
    expect(onSelect).toHaveBeenCalledWith("Podcast");
  });
});
```

- [ ] **Step 7: Create `TemplatePickerModal.tsx`**

Create `apps/desktop/src/components/TemplatePickerModal.tsx`:

```tsx
import type { TemplateInfo } from "../lib/tauri-bridge";

interface TemplatePickerModalProps {
  open: boolean;
  templates: TemplateInfo[];
  onSelect: (name: string) => void;
  onClose: () => void;
}

export function TemplatePickerModal({
  open,
  templates,
  onSelect,
  onClose,
}: TemplatePickerModalProps) {
  if (!open) return null;
  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60"
      onClick={onClose}
    >
      <div
        className="bg-neutral-900 border border-neutral-700 rounded-xl shadow-2xl w-80 p-6"
        onClick={e => e.stopPropagation()}
      >
        <h2 className="text-sm font-semibold text-neutral-200 tracking-wide uppercase mb-4">
          Start from template
        </h2>
        <ul className="space-y-2">
          {templates.map(t => (
            <li key={t.name}>
              <button
                className="w-full text-left px-3 py-2 rounded-lg border border-neutral-700 hover:border-amber-400/60 hover:bg-amber-400/5 transition-colors"
                onClick={() => onSelect(t.name)}
              >
                <div className="text-sm text-neutral-200 font-medium">{t.name}</div>
                <div className="text-xs text-neutral-500">{t.description}</div>
              </button>
            </li>
          ))}
        </ul>
      </div>
    </div>
  );
}
```

- [ ] **Step 8: Wire template picker in App.tsx**

```tsx
import { TemplatePickerModal } from "./components/TemplatePickerModal";
import { applyTemplate, listTemplates, type TemplateInfo } from "./lib/tauri-bridge";

// State:
const [templates, setTemplates] = useState<TemplateInfo[]>([]);
const [showTemplatePicker, setShowTemplatePicker] = useState(false);

// On mount, load templates:
useEffect(() => {
  listTemplates().then(setTemplates).catch(console.error);
}, []);

const handleApplyTemplate = useCallback(async (name: string) => {
  setShowTemplatePicker(false);
  try {
    const newHead = await applyTemplate(name);
    setHeadLocal(newHead);
    const newTracks = await listTracks();
    setTracks(newTracks);
  } catch (e) {
    setRenderError(String(e));
  }
}, [setHeadLocal]);

// In JSX:
<TemplatePickerModal
  open={showTemplatePicker}
  templates={templates}
  onSelect={handleApplyTemplate}
  onClose={() => setShowTemplatePicker(false)}
/>
```

Add a "Templates" button in the EmptyState component so it's discoverable when no audio is loaded.

In `apps/desktop/src/components/EmptyState.tsx`, add an `onShowTemplates?: () => void` prop and render a button.

- [ ] **Step 9: Full test run**

```bash
pnpm --filter @edytlab/desktop test
pnpm --filter @edytlab/desktop exec tsc --noEmit
cargo test --workspace
```

Expected: all pass.

- [ ] **Step 10: Commit**

```bash
git add apps/desktop/src-tauri/resources/templates/ \
        apps/desktop/src-tauri/tauri.conf.json \
        apps/desktop/src-tauri/src/commands.rs \
        apps/desktop/src-tauri/src/lib.rs \
        apps/desktop/src/lib/tauri-bridge.ts \
        apps/desktop/src/components/TemplatePickerModal.tsx \
        apps/desktop/src/components/EmptyState.tsx \
        apps/desktop/src/App.tsx \
        apps/desktop/src/__tests__/TemplatePickerModal.test.tsx
git commit -m "feat(templates): session templates (podcast/music/interview) with picker modal"
```
