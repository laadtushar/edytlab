# Quick Wins Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add four high-ROI UX improvements — waveform zoom, undo/redo via DAG traversal, range-based export, and a keyboard shortcut overlay.

**Architecture:** All four features live primarily in the React frontend (`apps/desktop/src/`). Waveform zoom calls WaveSurfer's `zoom(pxPerSec)` API. Undo/redo walks the content-addressed DAG already persisted in the Rust `session::Store` — no new backend storage needed. Export selection adds one new Tauri command (`render_range`) and extends the `render_final` agent tool. The shortcut overlay is a pure-React modal.

**Tech Stack:** React 19, TypeScript, WaveSurfer.js 7, Tauri 2 IPC (`invoke`), Rust (audio-engine crate), `hound` for WAV writing.

---

## File Map

| File | Change |
|------|--------|
| `apps/desktop/src/App.tsx` | Add `zoom`, `zoomPxPerSec`, `redoStack` state; Ctrl+Z/Y, +/−/0, `?` key handlers; pass `zoom` prop to Timeline |
| `apps/desktop/src/components/Timeline.tsx` | Add `zoom?: number` + `onZoomChange?: (z: number) => void` props; pass `zoom` to all `TrackLane`; wire scroll handler |
| `apps/desktop/src/components/ShortcutsOverlay.tsx` | New component — keyboard shortcut reference modal |
| `apps/desktop/src/lib/tauri-bridge.ts` | Add `renderRange(nodeId, startSec, endSec, outPath)` |
| `apps/desktop/src-tauri/src/commands.rs` | Add `render_range` Tauri command |
| `apps/desktop/src-tauri/src/lib.rs` | Register `render_range` in `invoke_handler` |

---

### Task 1: Waveform Zoom

**Files:**
- Modify: `apps/desktop/src/components/Timeline.tsx`
- Modify: `apps/desktop/src/App.tsx`

WaveSurfer 7 exposes `wavesurfer.zoom(pxPerSec: number)`. At `pxPerSec = 0` WaveSurfer auto-fits the waveform to its container width. Higher values zoom in and the container scrolls horizontally. Each `TrackLane` owns its own WaveSurfer instance, so all lanes must receive and apply the same zoom value.

- [ ] **Step 1: Add `zoom` prop to `TrackLane`**

In `apps/desktop/src/components/Timeline.tsx`, extend `LaneProps` and wire the prop to WaveSurfer:

```tsx
// In LaneProps interface (around line 81), add:
zoom?: number;

// In the TrackLane function body, add a useEffect after the existing wsRef effects:
useEffect(() => {
  if (!wsRef.current) return;
  wsRef.current.zoom(zoom ?? 0);
}, [zoom]);
```

The WaveSurfer container also needs horizontal scroll enabled. Locate the `waveformWrapperRef` div and add `overflow-x: auto` via Tailwind:

```tsx
// Find the div that uses waveformWrapperRef and add the class:
<div ref={waveformWrapperRef} className="flex-1 overflow-x-auto" ...>
```

- [ ] **Step 2: Verify TrackLane compiles**

```bash
pnpm --filter @edytlab/desktop exec tsc --noEmit
```

Expected: no new errors.

- [ ] **Step 3: Add `zoom` and `onZoomChange` props to `Timeline`**

In `TimelineProps` (around line 65):

```tsx
zoom?: number;
onZoomChange?: (zoom: number) => void;
```

Pass `zoom` down to every `TrackLane` render. Locate where `TrackLane` is rendered (search for `<TrackLane`) and add the prop:

```tsx
<TrackLane
  key={t.audioPath}
  name={t.name}
  audioPath={t.audioPath || null}
  muted={t.muted}
  zoom={zoom}
  ...
/>
```

Also add a scroll/wheel handler on the outer Timeline container to let Ctrl+Scroll change zoom:

```tsx
const handleWheel = useCallback(
  (e: React.WheelEvent) => {
    if (!e.ctrlKey) return;
    e.preventDefault();
    const delta = e.deltaY > 0 ? -20 : 20;
    onZoomChange?.(Math.max(0, (zoom ?? 0) + delta));
  },
  [zoom, onZoomChange],
);

// Apply to the outermost Timeline div:
<div ... onWheel={handleWheel}>
```

- [ ] **Step 4: Add zoom state to App**

In `apps/desktop/src/App.tsx`, after the existing state declarations (around line 80):

```tsx
const [zoomPxPerSec, setZoomPxPerSec] = useState(0);
```

Pass to Timeline:

```tsx
<Timeline
  ref={timelineRef}
  ...
  zoom={zoomPxPerSec}
  onZoomChange={setZoomPxPerSec}
/>
```

Add keyboard handlers in the existing `keydown` `useEffect` (where Space/Home/End are handled):

```tsx
// Inside the existing handleKey function:
if (e.key === "+" || e.key === "=") {
  e.preventDefault();
  setZoomPxPerSec(z => Math.min(z + 40, 2000));
}
if (e.key === "-") {
  e.preventDefault();
  setZoomPxPerSec(z => Math.max(z - 40, 0));
}
if (e.key === "0") {
  e.preventDefault();
  setZoomPxPerSec(0);
}
```

- [ ] **Step 5: Run frontend tests and type check**

```bash
pnpm --filter @edytlab/desktop test
pnpm --filter @edytlab/desktop exec tsc --noEmit
```

Expected: all pass.

- [ ] **Step 6: Commit**

```bash
git add apps/desktop/src/components/Timeline.tsx apps/desktop/src/App.tsx
git commit -m "feat(timeline): waveform zoom via Ctrl+scroll and +/-/0 keys"
```

---

### Task 2: Undo / Redo via DAG

**Files:**
- Modify: `apps/desktop/src/App.tsx`

The session DAG is content-addressed and immutable. Every node stores its `parent` id. "Undo" means walking to the parent node; "redo" means returning to a previously visited child. We track the redo stack in React state. Any new node creation (agent tool call) clears the redo stack because the forward history is no longer linear.

`useSession()` already exposes `setHeadLocal(id)` to update the frontend's head pointer without an IPC round-trip. After calling the Tauri `set_head_to` command we also call `setHeadLocal` to keep state in sync.

- [ ] **Step 1: Write the failing test for undo state logic**

In `apps/desktop/src/__tests__/App.undoRedo.test.ts` (new file):

```ts
import { describe, expect, it, vi } from "vitest";

// Pure logic test — no DOM, no Tauri
function applyUndo(
  head: string,
  parent: string | null,
  redoStack: string[],
): { head: string; redoStack: string[] } | null {
  if (!parent) return null;
  return { head: parent, redoStack: [...redoStack, head] };
}

function applyRedo(
  redoStack: string[],
): { head: string; redoStack: string[] } | null {
  if (redoStack.length === 0) return null;
  const next = redoStack[redoStack.length - 1];
  return { head: next, redoStack: redoStack.slice(0, -1) };
}

describe("undo/redo logic", () => {
  it("undo pushes current head to redo stack and returns parent", () => {
    const result = applyUndo("node-b", "node-a", []);
    expect(result).toEqual({ head: "node-a", redoStack: ["node-b"] });
  });

  it("undo at root (no parent) returns null", () => {
    expect(applyUndo("node-a", null, [])).toBeNull();
  });

  it("redo pops from redo stack", () => {
    const result = applyRedo(["node-b"]);
    expect(result).toEqual({ head: "node-b", redoStack: [] });
  });

  it("redo on empty stack returns null", () => {
    expect(applyRedo([])).toBeNull();
  });

  it("undo then redo returns to original head", () => {
    const afterUndo = applyUndo("node-b", "node-a", [])!;
    const afterRedo = applyRedo(afterUndo.redoStack)!;
    expect(afterRedo.head).toBe("node-b");
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

```bash
pnpm --filter @edytlab/desktop test -- --run App.undoRedo
```

Expected: FAIL — file not found / functions undefined.

- [ ] **Step 3: Extract the logic into a helper and make the test pass**

Create `apps/desktop/src/lib/undoRedo.ts`:

```ts
export function applyUndo(
  head: string,
  parent: string | null,
  redoStack: string[],
): { head: string; redoStack: string[] } | null {
  if (!parent) return null;
  return { head: parent, redoStack: [...redoStack, head] };
}

export function applyRedo(
  redoStack: string[],
): { head: string; redoStack: string[] } | null {
  if (redoStack.length === 0) return null;
  const next = redoStack[redoStack.length - 1];
  return { head: next, redoStack: redoStack.slice(0, -1) };
}
```

Update the test import:

```ts
import { applyUndo, applyRedo } from "../lib/undoRedo";
```

- [ ] **Step 4: Run test to verify it passes**

```bash
pnpm --filter @edytlab/desktop test -- --run App.undoRedo
```

Expected: 5/5 PASS.

- [ ] **Step 5: Wire undo/redo into App.tsx**

In `apps/desktop/src/App.tsx`:

Add state and imports:

```tsx
import { applyUndo, applyRedo } from "./lib/undoRedo";
import { getNode, setHeadTo } from "./lib/tauri-bridge";

// After existing state declarations:
const [redoStack, setRedoStack] = useState<string[]>([]);
```

Add handlers (outside the JSX return, inside the `App` function):

```tsx
const handleUndo = useCallback(async () => {
  if (!head) return;
  const node = await getNode(head);
  const result = applyUndo(head, node.parent ?? null, redoStack);
  if (!result) return;
  await setHeadTo(result.head);
  setHeadLocal(result.head);
  setRedoStack(result.redoStack);
  const newTracks = await listTracks();
  setTracks(newTracks);
}, [head, redoStack, setHeadLocal]);

const handleRedo = useCallback(async () => {
  const result = applyRedo(redoStack);
  if (!result) return;
  await setHeadTo(result.head);
  setHeadLocal(result.head);
  setRedoStack(result.redoStack);
  const newTracks = await listTracks();
  setTracks(newTracks);
}, [redoStack, setHeadLocal]);
```

Clear redo stack on new node creation — add to the existing `onNodeCreated` `useEffect`:

```tsx
// Inside the useEffect that calls onNodeCreated:
const unsub = onNodeCreated(async (nodeId: string) => {
  setRedoStack([]); // new branch clears forward history
  setGraphRefresh(n => n + 1);
  const newTracks = await listTracks();
  setTracks(newTracks);
});
```

Add Ctrl+Z / Ctrl+Y keyboard handlers in the existing `keydown` `useEffect`:

```tsx
// Inside handleKey, before the Space handler:
if (e.ctrlKey && !e.shiftKey && e.key === "z") {
  e.preventDefault();
  handleUndo();
  return;
}
if (
  (e.ctrlKey && e.key === "y") ||
  (e.ctrlKey && e.shiftKey && e.key === "z")
) {
  e.preventDefault();
  handleRedo();
  return;
}
```

The `useEffect` dependency array must include `handleUndo` and `handleRedo`. Update accordingly.

- [ ] **Step 6: Type check and test**

```bash
pnpm --filter @edytlab/desktop exec tsc --noEmit
pnpm --filter @edytlab/desktop test
```

Expected: all pass.

- [ ] **Step 7: Commit**

```bash
git add apps/desktop/src/lib/undoRedo.ts \
        apps/desktop/src/__tests__/App.undoRedo.test.ts \
        apps/desktop/src/App.tsx
git commit -m "feat(app): undo/redo via DAG parent traversal (Ctrl+Z / Ctrl+Y)"
```

---

### Task 3: Export Selection (Range-Based Render)

**Files:**
- Create: `apps/desktop/src-tauri/src/commands.rs` (add `render_range`)
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Modify: `apps/desktop/src/lib/tauri-bridge.ts`
- Modify: `apps/desktop/src/components/Chat.tsx` (add Export Selection button)

`render_final` currently passes `None` for `TimeRange`. We add a new `render_range` Tauri command that accepts seconds, converts to sample frames, and delegates to the same `Engine::render_to_wav`.

- [ ] **Step 1: Write a Rust test for the frame conversion helper**

In `apps/desktop/src-tauri/src/commands.rs`, after the existing test module (search `#[cfg(test)]`):

```rust
#[cfg(test)]
mod render_range_tests {
    #[test]
    fn sec_to_frame_conversion() {
        // 44100 Hz, start 1.0s, end 2.5s
        let sample_rate: u32 = 44100;
        let start_frame = (1.0_f64 * sample_rate as f64) as u64;
        let end_frame = (2.5_f64 * sample_rate as f64) as u64;
        assert_eq!(start_frame, 44100);
        assert_eq!(end_frame, 110250);
    }
}
```

- [ ] **Step 2: Run test to verify it passes (pure arithmetic, no deps)**

```bash
cargo test --package desktop-tauri render_range_tests
```

Expected: PASS.

- [ ] **Step 3: Add `render_range` command to `commands.rs`**

Find the end of the commands block in `apps/desktop/src-tauri/src/commands.rs` and add:

```rust
#[tauri::command]
pub async fn render_range(
    state: tauri::State<'_, AppState>,
    node_id: String,
    start_sec: f64,
    end_sec: f64,
    out_path: String,
) -> Result<serde_json::Value, CommandError> {
    let (store, engine) = {
        let s = lock_std(&state.store, "store")?;
        let e = lock_std(&state.engine, "engine")?;
        (s, e)
    };
    let node_id = session::NodeId::from_hex(&node_id)
        .map_err(|_| CommandError::InvalidNodeId)?;
    let node = store.get(node_id).map_err(CommandError::Session)?;
    let sample_rate = node.state.sample_rate;
    let start_frame = (start_sec * sample_rate as f64) as u64;
    let end_frame = (end_sec * sample_rate as f64) as u64;
    let range = audio_engine::TimeRange { start_frame, end_frame };
    let out = std::path::PathBuf::from(&out_path);
    let report = engine
        .render_to_wav(&node.state, &out, Some(range))
        .map_err(CommandError::Engine)?;
    Ok(serde_json::json!({
        "path": out_path,
        "frames_written": report.frames_written,
        "sample_rate": report.sample_rate,
        "channels": report.channels,
        "peak_dbfs": report.peak_dbfs,
        "summary": format!(
            "Exported selection ({:.2}s–{:.2}s) → {}",
            start_sec, end_sec, out_path
        ),
    }))
}
```

Note: `lock_std` locks and immediately drops — you need separate locks because the borrow checker requires distinct variables. If `AppState` uses a single `Mutex<(Store, Engine)>`, adjust accordingly. The pattern matches what other commands do.

- [ ] **Step 4: Register in `lib.rs`**

In `apps/desktop/src-tauri/src/lib.rs`, add `render_range` to the `invoke_handler`:

```rust
// Find the .invoke_handler(tauri::generate_handler![...]) call and add:
commands::render_range,
```

- [ ] **Step 5: Verify Rust compiles**

```bash
cargo build --package desktop-tauri 2>&1 | head -40
```

Expected: no errors.

- [ ] **Step 6: Add `renderRange` to `tauri-bridge.ts`**

In `apps/desktop/src/lib/tauri-bridge.ts`, add after the existing `renderPreview` function:

```ts
export async function renderRange(
  nodeId: string,
  startSec: number,
  endSec: number,
  outPath: string,
): Promise<Record<string, unknown>> {
  return invoke("render_range", {
    nodeId,
    startSec,
    endSec,
    outPath,
  });
}
```

- [ ] **Step 7: Add Export Selection button to Chat header**

In `apps/desktop/src/components/Chat.tsx`, find `ChatHeader` (the inner component that renders the render preview button). Add an "Export Selection" button that's only visible when `selection` is non-null:

```tsx
// ChatHeader receives selection prop — check the existing ChatProps:
// selection?: { start: number; end: number } | null

// Inside ChatHeader, add alongside the existing render button:
{props.selection && (
  <button
    data-testid="export-selection-btn"
    onClick={props.onExportSelection}
    className="text-xs text-amber-400 border border-amber-400/40 rounded px-2 py-1 hover:bg-amber-400/10 transition-colors"
  >
    Export Selection
  </button>
)}
```

Add `onExportSelection?: () => void` to `ChatProps`.

- [ ] **Step 8: Wire export in App.tsx**

In `apps/desktop/src/App.tsx`:

```tsx
import { renderRange } from "./lib/tauri-bridge";
import { save } from "@tauri-apps/plugin-dialog";

const handleExportSelection = useCallback(async () => {
  if (!head || !selection) return;
  const outPath = await save({
    title: "Export Selection",
    filters: [{ name: "WAV", extensions: ["wav"] }],
    defaultPath: "export.wav",
  });
  if (!outPath) return;
  try {
    await renderRange(head, selection.start, selection.end, outPath);
  } catch (e) {
    setRenderError(String(e));
  }
}, [head, selection]);

// Pass to Chat:
<Chat
  ...
  onExportSelection={handleExportSelection}
/>
```

- [ ] **Step 9: Type check**

```bash
pnpm --filter @edytlab/desktop exec tsc --noEmit
```

Expected: clean.

- [ ] **Step 10: Commit**

```bash
git add apps/desktop/src-tauri/src/commands.rs \
        apps/desktop/src-tauri/src/lib.rs \
        apps/desktop/src/lib/tauri-bridge.ts \
        apps/desktop/src/components/Chat.tsx \
        apps/desktop/src/App.tsx
git commit -m "feat(export): render_range command + Export Selection button in chat header"
```

---

### Task 4: Keyboard Shortcut Overlay

**Files:**
- Create: `apps/desktop/src/components/ShortcutsOverlay.tsx`
- Modify: `apps/desktop/src/App.tsx`

- [ ] **Step 1: Write a smoke test for the overlay**

In `apps/desktop/src/__tests__/ShortcutsOverlay.test.tsx` (new file):

```tsx
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { ShortcutsOverlay } from "../components/ShortcutsOverlay";

describe("ShortcutsOverlay", () => {
  it("renders nothing when closed", () => {
    const { container } = render(
      <ShortcutsOverlay open={false} onClose={() => {}} />,
    );
    expect(container).toBeEmptyDOMElement();
  });

  it("renders shortcut list when open", () => {
    render(<ShortcutsOverlay open={true} onClose={() => {}} />);
    expect(screen.getByText("Space")).toBeInTheDocument();
    expect(screen.getByText("Ctrl+Z")).toBeInTheDocument();
    expect(screen.getByText("?")).toBeInTheDocument();
  });

  it("calls onClose when Escape pressed", async () => {
    const onClose = vi.fn();
    render(<ShortcutsOverlay open={true} onClose={onClose} />);
    const user = userEvent.setup();
    await user.keyboard("{Escape}");
    expect(onClose).toHaveBeenCalled();
  });
});
```

Add `import userEvent from "@testing-library/user-event";` and `import { vi } from "vitest";` at the top.

- [ ] **Step 2: Run test to verify it fails**

```bash
pnpm --filter @edytlab/desktop test -- --run ShortcutsOverlay
```

Expected: FAIL — component not found.

- [ ] **Step 3: Create `ShortcutsOverlay.tsx`**

Create `apps/desktop/src/components/ShortcutsOverlay.tsx`:

```tsx
import { useEffect } from "react";

interface Shortcut {
  keys: string;
  description: string;
}

const SHORTCUTS: Shortcut[] = [
  { keys: "Space", description: "Play / Pause" },
  { keys: "Home", description: "Seek to start" },
  { keys: "End", description: "Seek to end" },
  { keys: "← →", description: "Seek ±5 seconds" },
  { keys: "Shift+← →", description: "Seek ±1 second" },
  { keys: "Escape", description: "Clear selection" },
  { keys: "Ctrl+Z", description: "Undo" },
  { keys: "Ctrl+Y / Ctrl+Shift+Z", description: "Redo" },
  { keys: "+ / =", description: "Zoom in" },
  { keys: "-", description: "Zoom out" },
  { keys: "0", description: "Reset zoom" },
  { keys: "?", description: "Show this overlay" },
];

interface ShortcutsOverlayProps {
  open: boolean;
  onClose: () => void;
}

export function ShortcutsOverlay({ open, onClose }: ShortcutsOverlayProps) {
  useEffect(() => {
    if (!open) return;
    function handleKey(e: KeyboardEvent) {
      if (e.key === "Escape") onClose();
    }
    window.addEventListener("keydown", handleKey);
    return () => window.removeEventListener("keydown", handleKey);
  }, [open, onClose]);

  if (!open) return null;

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60"
      onClick={onClose}
      data-testid="shortcuts-overlay"
    >
      <div
        className="bg-neutral-900 border border-neutral-700 rounded-xl shadow-2xl w-96 p-6"
        onClick={e => e.stopPropagation()}
      >
        <div className="flex items-center justify-between mb-4">
          <h2 className="text-sm font-semibold text-neutral-200 tracking-wide uppercase">
            Keyboard Shortcuts
          </h2>
          <button
            onClick={onClose}
            className="text-neutral-500 hover:text-neutral-300 text-lg leading-none"
            aria-label="Close"
          >
            ×
          </button>
        </div>
        <table className="w-full text-sm">
          <tbody>
            {SHORTCUTS.map(({ keys, description }) => (
              <tr key={keys} className="border-b border-neutral-800 last:border-0">
                <td className="py-2 pr-4 font-mono text-amber-400 whitespace-nowrap">
                  {keys}
                </td>
                <td className="py-2 text-neutral-300">{description}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
}
```

- [ ] **Step 4: Run test to verify it passes**

```bash
pnpm --filter @edytlab/desktop test -- --run ShortcutsOverlay
```

Expected: 3/3 PASS.

- [ ] **Step 5: Wire into App.tsx**

In `apps/desktop/src/App.tsx`:

```tsx
import { ShortcutsOverlay } from "./components/ShortcutsOverlay";

// State:
const [showShortcuts, setShowShortcuts] = useState(false);

// In the existing keydown handler, add before the Space check:
if (e.key === "?" && !e.ctrlKey && !e.altKey && !e.metaKey) {
  e.preventDefault();
  setShowShortcuts(v => !v);
  return;
}

// In JSX, add alongside other modals (Settings, ABCompareBar):
<ShortcutsOverlay open={showShortcuts} onClose={() => setShowShortcuts(false)} />
```

- [ ] **Step 6: Type check and full test run**

```bash
pnpm --filter @edytlab/desktop exec tsc --noEmit
pnpm --filter @edytlab/desktop test
cargo test --workspace
```

Expected: all pass.

- [ ] **Step 7: Commit**

```bash
git add apps/desktop/src/components/ShortcutsOverlay.tsx \
        apps/desktop/src/__tests__/ShortcutsOverlay.test.tsx \
        apps/desktop/src/App.tsx
git commit -m "feat(shortcuts): keyboard shortcut overlay on ? key"
```
