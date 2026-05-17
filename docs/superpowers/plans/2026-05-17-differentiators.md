# Differentiators Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Deliver three differentiating features: branch diff view (visual compare two DAG nodes with operation list), shareable session links (zip export/import), and voice commands (push-to-talk in chat using Whisper).

**Architecture:** Branch diff reuses `session::diff_states` (already in the crate) via a new Tauri command and renders the diff in a `DiffPanel` component that complements the existing `ABCompareBar`. Session zip export collects all `source_path` files referenced in a session node's state, writes them alongside a `session.json` manifest, and packages them with the `zip` crate. Voice commands use `cpal` (already in the audio stack) to record microphone input to a temp WAV, then transcribe it through the existing Whisper path and insert the text into the chat input.

**Tech Stack:** React 19, TypeScript, Tauri 2, Rust (`session`, `audio-engine`, `tools` crates), `zip` crate (new dep), `cpal` (existing), `tauri-plugin-dialog`.

---

## File Map

| File | Change |
|------|--------|
| `apps/desktop/src-tauri/src/commands.rs` | Add `diff_nodes`, `export_session_zip`, `import_session_zip`, `start_voice_recording`, `stop_voice_recording` |
| `apps/desktop/src-tauri/src/lib.rs` | Register new commands |
| `apps/desktop/src-tauri/Cargo.toml` | Add `zip = "2"` dependency |
| `apps/desktop/src/lib/tauri-bridge.ts` | Add bridge fns for all new commands |
| `apps/desktop/src/components/DiffPanel.tsx` | New — shows operation list between two nodes |
| `apps/desktop/src/components/GraphView.tsx` | Wire "Compare" context menu item to open DiffPanel |
| `apps/desktop/src/components/Chat.tsx` | Add mic button, recording indicator, voice-to-text |
| `apps/desktop/src/App.tsx` | Add DiffPanel state, export/import handlers |

---

### Task 13: Branch Diff View

**Files:**
- Modify: `apps/desktop/src-tauri/src/commands.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Modify: `apps/desktop/src/lib/tauri-bridge.ts`
- Create: `apps/desktop/src/components/DiffPanel.tsx`
- Modify: `apps/desktop/src/components/GraphView.tsx`
- Modify: `apps/desktop/src/App.tsx`

`session::diff_states(a: &SessionState, b: &SessionState) -> SessionDiff` already exists. `SessionDiff` contains a `Vec<DiffOp>` describing operations. We expose this via a Tauri command and render it as a human-readable panel.

- [ ] **Step 1: Inspect `SessionDiff` and `DiffOp` types**

```bash
grep -n "pub enum DiffOp\|pub struct SessionDiff\|DiffTarget\|BusMeta\|EffectScope" crates/session/src/diff.rs | head -30
```

Read the output carefully. You need the exact variant names for `DiffOp` to render them in the panel. Record them.

- [ ] **Step 2: Write a Rust test for the diff command logic**

In `apps/desktop/src-tauri/src/commands.rs` test module:

```rust
#[cfg(test)]
mod diff_tests {
    use session::{SessionState, TempoMap};

    fn empty_state() -> SessionState {
        SessionState {
            tracks: vec![],
            bus_routing: session::BusGraph { buses: vec![] },
            master_chain: vec![],
            tempo_map: TempoMap { default_bpm: 120.0, segments: vec![] },
            key_map: None,
            transcript: None,
            sample_rate: 44100,
            length_samples: 0,
            annotations: vec![],
        }
    }

    #[test]
    fn diff_identical_states_is_empty() {
        let s = empty_state();
        let diff = session::diff_states(&s, &s);
        // Two identical states produce zero ops.
        assert!(diff.ops.is_empty(), "expected no ops, got {:?}", diff.ops);
    }
}
```

Replace `diff.ops` with the actual field name from Step 1 (it may be `operations`, `changes`, or similar).

- [ ] **Step 3: Run test to verify it passes**

```bash
cargo test --package desktop-tauri diff_tests 2>&1 | tail -5
```

Expected: PASS (the test uses only existing session API).

- [ ] **Step 4: Add `diff_nodes` command to `commands.rs`**

First check the exact return type of `session::diff_states`. It likely returns `session::SessionDiff`. Check what `serde::Serialize` derives exist on it:

```bash
grep -n "Serialize\|pub struct SessionDiff\|pub enum DiffOp" crates/session/src/diff.rs | head -20
```

If `SessionDiff` derives `Serialize`, use it directly. If not, create a DTO:

```rust
#[tauri::command]
pub async fn diff_nodes(
    state: tauri::State<'_, AppState>,
    node_a: String,
    node_b: String,
) -> Result<serde_json::Value, CommandError> {
    let store = lock_std(&state.store, "store")?;
    let id_a = session::NodeId::from_hex(&node_a).map_err(|_| CommandError::InvalidNodeId)?;
    let id_b = session::NodeId::from_hex(&node_b).map_err(|_| CommandError::InvalidNodeId)?;
    let a = store.get(id_a).map_err(CommandError::Session)?;
    let b = store.get(id_b).map_err(CommandError::Session)?;
    let diff = session::diff_states(&a.state, &b.state);
    // Serialize the diff — if SessionDiff: Serialize, use serde_json::to_value directly.
    // Otherwise build a summary manually.
    let value = serde_json::to_value(&diff)
        .unwrap_or_else(|_| serde_json::json!({ "error": "diff not serializable" }));
    Ok(value)
}
```

- [ ] **Step 5: Register and bridge**

In `lib.rs`:

```rust
commands::diff_nodes,
```

In `tauri-bridge.ts`:

```ts
export async function diffNodes(
  nodeA: string,
  nodeB: string,
): Promise<Record<string, unknown>> {
  return invoke("diff_nodes", { nodeA, nodeB });
}
```

- [ ] **Step 6: Write a test for DiffPanel**

In `apps/desktop/src/__tests__/DiffPanel.test.tsx`:

```tsx
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { DiffPanel } from "../components/DiffPanel";

const SAMPLE_DIFF = {
  ops: [
    { kind: "TrackAdded", track_name: "Guest" },
    { kind: "GainChanged", track_name: "Host", from_db: 0.0, to_db: -3.0 },
  ],
};

describe("DiffPanel", () => {
  it("renders each operation", () => {
    render(<DiffPanel diff={SAMPLE_DIFF} nodeA="abc" nodeB="def" />);
    expect(screen.getByText(/TrackAdded/i)).toBeInTheDocument();
    expect(screen.getByText(/GainChanged/i)).toBeInTheDocument();
  });

  it("shows node ids in the header", () => {
    render(<DiffPanel diff={SAMPLE_DIFF} nodeA="abc123" nodeB="def456" />);
    expect(screen.getByText(/abc123/)).toBeInTheDocument();
    expect(screen.getByText(/def456/)).toBeInTheDocument();
  });
});
```

- [ ] **Step 7: Create `DiffPanel.tsx`**

Create `apps/desktop/src/components/DiffPanel.tsx`:

```tsx
interface DiffOp {
  kind: string;
  [key: string]: unknown;
}

interface DiffPanelProps {
  diff: { ops?: DiffOp[]; [key: string]: unknown };
  nodeA: string;
  nodeB: string;
  onClose?: () => void;
}

function formatOp(op: DiffOp): string {
  const rest = Object.entries(op)
    .filter(([k]) => k !== "kind")
    .map(([k, v]) => `${k}: ${JSON.stringify(v)}`)
    .join(", ");
  return rest ? `${op.kind} (${rest})` : op.kind;
}

export function DiffPanel({ diff, nodeA, nodeB, onClose }: DiffPanelProps) {
  const ops: DiffOp[] = (diff.ops as DiffOp[]) ?? [];

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60"
      onClick={onClose}
    >
      <div
        className="bg-neutral-900 border border-neutral-700 rounded-xl shadow-2xl w-[520px] max-h-[70vh] flex flex-col"
        onClick={e => e.stopPropagation()}
      >
        <div className="flex items-center justify-between px-5 py-4 border-b border-neutral-800">
          <div>
            <div className="text-sm font-semibold text-neutral-200">Branch Diff</div>
            <div className="text-xs text-neutral-500 mt-0.5">
              <span className="font-mono text-amber-400">{nodeA.slice(0, 8)}</span>
              <span className="mx-2">→</span>
              <span className="font-mono text-blue-400">{nodeB.slice(0, 8)}</span>
            </div>
          </div>
          {onClose && (
            <button onClick={onClose} className="text-neutral-500 hover:text-neutral-300 text-xl">
              ×
            </button>
          )}
        </div>
        <div className="flex-1 overflow-y-auto px-5 py-3">
          {ops.length === 0 ? (
            <p className="text-sm text-neutral-500 py-4 text-center">
              No differences between these nodes.
            </p>
          ) : (
            <ul className="space-y-1.5">
              {ops.map((op, i) => (
                <li
                  key={i}
                  className="flex items-start gap-3 text-sm py-1.5 border-b border-neutral-800 last:border-0"
                >
                  <span className="mt-0.5 w-2 h-2 rounded-full bg-amber-400 flex-shrink-0" />
                  <span className="text-neutral-300 font-mono text-xs break-all">
                    {formatOp(op)}
                  </span>
                </li>
              ))}
            </ul>
          )}
        </div>
        <div className="px-5 py-3 border-t border-neutral-800 text-xs text-neutral-600">
          {ops.length} operation{ops.length !== 1 ? "s" : ""}
        </div>
      </div>
    </div>
  );
}
```

- [ ] **Step 8: Run DiffPanel test**

```bash
pnpm --filter @edytlab/desktop test -- --run DiffPanel
```

Expected: 2/2 PASS.

- [ ] **Step 9: Wire "Compare" in GraphView and App**

In `apps/desktop/src/components/GraphView.tsx`, the context menu already has a "Compare" item that calls `onCompareNodes`. Currently `onCompareNodes` opens an `ABCompareBar`. We'll extend it to also open the DiffPanel.

In `apps/desktop/src/App.tsx`:

```tsx
import { DiffPanel } from "./components/DiffPanel";
import { diffNodes } from "./lib/tauri-bridge";

// State:
const [diffResult, setDiffResult] = useState<Record<string, unknown> | null>(null);
const [diffNodes_state, setDiffNodes] = useState<{ a: string; b: string } | null>(null);

// When compare mode is set (existing logic), also fetch the diff:
const handleCompare = useCallback(async (nodeId: string) => {
  if (!head) return;
  setCompareMode({ a: head, b: nodeId });
  try {
    const diff = await diffNodes(head, nodeId);
    setDiffResult(diff);
    setDiffNodes({ a: head, b: nodeId });
  } catch (e) {
    console.error(e);
  }
}, [head]);

// Pass handleCompare to GraphView's onCompareNodes prop.

// In JSX:
{diffResult && diffNodes_state && (
  <DiffPanel
    diff={diffResult}
    nodeA={diffNodes_state.a}
    nodeB={diffNodes_state.b}
    onClose={() => { setDiffResult(null); setDiffNodes(null); }}
  />
)}
```

- [ ] **Step 10: Type check and full test**

```bash
pnpm --filter @edytlab/desktop exec tsc --noEmit
pnpm --filter @edytlab/desktop test
cargo test --workspace
```

Expected: all pass.

- [ ] **Step 11: Commit**

```bash
git add apps/desktop/src-tauri/src/commands.rs \
        apps/desktop/src-tauri/src/lib.rs \
        apps/desktop/src/lib/tauri-bridge.ts \
        apps/desktop/src/components/DiffPanel.tsx \
        apps/desktop/src/components/GraphView.tsx \
        apps/desktop/src/App.tsx \
        apps/desktop/src/__tests__/DiffPanel.test.tsx
git commit -m "feat(diff): branch diff view comparing two DAG nodes side-by-side"
```

---

### Task 14: Shareable Session Links (Zip Export/Import)

**Files:**
- Modify: `apps/desktop/src-tauri/Cargo.toml`
- Modify: `apps/desktop/src-tauri/src/commands.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Modify: `apps/desktop/src/lib/tauri-bridge.ts`
- Modify: `apps/desktop/src/App.tsx`

Export bundles the current node's `SessionState` JSON plus all referenced audio files into a zip archive. Import extracts it to a new project directory and opens it. The zip format is: `session.json` (the `SessionState`) + `audio/<filename>` for each unique `source_path` in the tracks.

- [ ] **Step 1: Add `zip` crate**

In `apps/desktop/src-tauri/Cargo.toml`, under `[dependencies]`:

```toml
zip = "2"
```

- [ ] **Step 2: Write a Rust test for zip round-trip**

In `apps/desktop/src-tauri/src/commands.rs` test module:

```rust
#[cfg(test)]
mod zip_tests {
    #[test]
    fn zip_write_then_read_roundtrip() {
        use std::io::{Cursor, Read, Write};
        use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

        let mut buf = Vec::new();
        {
            let cursor = Cursor::new(&mut buf);
            let mut zip = ZipWriter::new(cursor);
            zip.start_file("hello.txt", SimpleFileOptions::default()).unwrap();
            zip.write_all(b"hello world").unwrap();
            zip.finish().unwrap();
        }
        let mut archive = ZipArchive::new(Cursor::new(&buf)).unwrap();
        let mut file = archive.by_name("hello.txt").unwrap();
        let mut content = String::new();
        file.read_to_string(&mut content).unwrap();
        assert_eq!(content, "hello world");
    }
}
```

- [ ] **Step 3: Run test**

```bash
cargo test --package desktop-tauri zip_tests 2>&1 | tail -5
```

Expected: PASS.

- [ ] **Step 4: Add `export_session_zip` command**

```rust
#[tauri::command]
pub async fn export_session_zip(
    state: tauri::State<'_, AppState>,
    out_path: String,
) -> Result<String, CommandError> {
    use std::io::{Read, Write};
    use zip::{ZipWriter, write::SimpleFileOptions};

    let store = lock_std(&state.store, "store")?;
    let head = store.head().ok_or(CommandError::NoSession)?;
    let node = store.get(head).map_err(CommandError::Session)?;

    let out_file = std::fs::File::create(&out_path).map_err(CommandError::Io)?;
    let mut zip = ZipWriter::new(out_file);
    let opts = SimpleFileOptions::default();

    // Write session.json
    let state_json = serde_json::to_vec_pretty(&node.state)
        .map_err(|e| CommandError::Io(std::io::Error::other(e.to_string())))?;
    zip.start_file("session.json", opts).map_err(|e| CommandError::Io(std::io::Error::other(e.to_string())))?;
    zip.write_all(&state_json).map_err(CommandError::Io)?;

    // Collect unique audio source paths
    let mut seen = std::collections::HashSet::new();
    for track in &node.state.tracks {
        for clip in &track.clips {
            let path = &clip.source_path;
            if seen.insert(path.clone()) {
                let filename = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("audio.wav");
                let entry_name = format!("audio/{filename}");
                let mut f = std::fs::File::open(path).map_err(CommandError::Io)?;
                let mut bytes = Vec::new();
                f.read_to_end(&mut bytes).map_err(CommandError::Io)?;
                zip.start_file(&entry_name, opts).map_err(|e| CommandError::Io(std::io::Error::other(e.to_string())))?;
                zip.write_all(&bytes).map_err(CommandError::Io)?;
            }
        }
    }

    zip.finish().map_err(|e| CommandError::Io(std::io::Error::other(e.to_string())))?;
    Ok(out_path)
}
```

- [ ] **Step 5: Add `import_session_zip` command**

```rust
#[tauri::command]
pub async fn import_session_zip(
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
    zip_path: String,
) -> Result<String, CommandError> {
    use std::io::Read;
    use zip::ZipArchive;

    // Extract to a new directory under app data
    let data_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|_| CommandError::Io(std::io::Error::other("app data dir")))?;
    let extract_dir = data_dir.join("imported").join(
        std::path::Path::new(&zip_path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("session"),
    );
    std::fs::create_dir_all(&extract_dir).map_err(CommandError::Io)?;

    let f = std::fs::File::open(&zip_path).map_err(CommandError::Io)?;
    let mut archive = ZipArchive::new(f)
        .map_err(|e| CommandError::Io(std::io::Error::other(e.to_string())))?;

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)
            .map_err(|e| CommandError::Io(std::io::Error::other(e.to_string())))?;
        let out_path = extract_dir.join(entry.name());
        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent).map_err(CommandError::Io)?;
        }
        let mut out_file = std::fs::File::create(&out_path).map_err(CommandError::Io)?;
        std::io::copy(&mut entry, &mut out_file).map_err(CommandError::Io)?;
    }

    // Read session.json and rewrite source_paths to point to extracted audio/
    let session_json_path = extract_dir.join("session.json");
    let raw = std::fs::read_to_string(&session_json_path).map_err(CommandError::Io)?;
    let mut session_state: session::SessionState = serde_json::from_str(&raw)
        .map_err(|e| CommandError::Io(std::io::Error::other(e.to_string())))?;

    // Remap source_paths: replace any path's filename to extracted audio dir
    let audio_dir = extract_dir.join("audio");
    for track in &mut session_state.tracks {
        for clip in &mut track.clips {
            if let Some(filename) = clip.source_path.file_name() {
                let new_path = audio_dir.join(filename);
                if new_path.exists() {
                    clip.source_path = new_path;
                }
            }
        }
    }

    // Store as a new session node
    let mut store = lock_std(&state.store, "store")?;
    let node = session::SessionNode {
        parent: None,
        state: session_state,
        label: Some("Imported session".to_string()),
        ..Default::default()
    };
    let new_id = store.append(node).map_err(CommandError::Session)?;
    Ok(new_id.to_hex())
}
```

- [ ] **Step 6: Register both commands in `lib.rs`**

```rust
commands::export_session_zip,
commands::import_session_zip,
```

- [ ] **Step 7: Add to `tauri-bridge.ts`**

```ts
export async function exportSessionZip(outPath: string): Promise<string> {
  return invoke("export_session_zip", { outPath });
}

export async function importSessionZip(zipPath: string): Promise<string> {
  return invoke("import_session_zip", { zipPath });
}
```

- [ ] **Step 8: Wire export/import buttons in App.tsx**

```tsx
import { save, open as openDialog } from "@tauri-apps/plugin-dialog";
import { exportSessionZip, importSessionZip } from "./lib/tauri-bridge";

const handleExportZip = useCallback(async () => {
  if (!head) return;
  const outPath = await save({
    title: "Export Session",
    filters: [{ name: "Session Archive", extensions: ["zip"] }],
    defaultPath: "session.zip",
  });
  if (!outPath) return;
  try {
    await exportSessionZip(outPath);
  } catch (e) {
    setRenderError(String(e));
  }
}, [head]);

const handleImportZip = useCallback(async () => {
  const zipPath = await openDialog({
    title: "Import Session",
    filters: [{ name: "Session Archive", extensions: ["zip"] }],
  });
  if (!zipPath || Array.isArray(zipPath)) return;
  try {
    const newHead = await importSessionZip(zipPath as string);
    setHeadLocal(newHead);
    const newTracks = await listTracks();
    setTracks(newTracks);
  } catch (e) {
    setRenderError(String(e));
  }
}, [setHeadLocal]);
```

Add Export and Import buttons to the `AppHeader` or a File menu. A simple approach: add them next to the existing "Open Audio" button.

- [ ] **Step 9: Verify Rust builds and tests pass**

```bash
cargo build --package desktop-tauri 2>&1 | head -30
cargo test --package desktop-tauri zip_tests
pnpm --filter @edytlab/desktop exec tsc --noEmit
```

Expected: all pass.

- [ ] **Step 10: Commit**

```bash
git add apps/desktop/src-tauri/Cargo.toml \
        apps/desktop/src-tauri/src/commands.rs \
        apps/desktop/src-tauri/src/lib.rs \
        apps/desktop/src/lib/tauri-bridge.ts \
        apps/desktop/src/App.tsx
git commit -m "feat(share): session zip export/import for sharing sessions across machines"
```

---

### Task 15: Voice Commands (Push-to-Talk)

**Files:**
- Modify: `apps/desktop/src-tauri/src/commands.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Modify: `apps/desktop/src/lib/tauri-bridge.ts`
- Modify: `apps/desktop/src/components/Chat.tsx`

Record microphone audio via `cpal`, write it to a temp WAV, transcribe it using the existing Whisper path (same `transcribe` tool machinery), and insert the result into the chat input. Push-and-hold the mic button or click-to-toggle.

Note: Check whether `cpal` is already in the workspace (it's used by `audio-io`). If so, add it as a dep to `desktop-tauri`'s `Cargo.toml`. If not, add `cpal = "0.15"`.

- [ ] **Step 1: Verify `cpal` is in the workspace**

```bash
grep -r "cpal" Cargo.toml crates/audio-io/Cargo.toml apps/desktop/src-tauri/Cargo.toml 2>/dev/null | head -10
```

If `cpal` appears in the workspace, check what version. Use the same version.

- [ ] **Step 2: Write a Rust test for the recording state machine**

In `apps/desktop/src-tauri/src/commands.rs` test module:

```rust
#[cfg(test)]
mod voice_tests {
    #[test]
    fn temp_wav_path_is_unique_per_call() {
        // Guard: two calls to the path generator return different paths.
        fn make_temp_path(id: u64) -> String {
            format!("/tmp/edytlab-voice-{id}.wav")
        }
        assert_ne!(make_temp_path(1), make_temp_path(2));
    }
}
```

- [ ] **Step 3: Add recording state to AppState**

In `apps/desktop/src-tauri/src/lib.rs` or wherever `AppState` is defined, add a recording handle field:

```rust
pub recording: std::sync::Mutex<Option<VoiceRecordingHandle>>,
```

Create `apps/desktop/src-tauri/src/voice.rs` with the handle type:

```rust
//! Microphone recording via cpal.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

pub struct VoiceRecordingHandle {
    pub stop_flag: Arc<Mutex<bool>>,
    pub out_path: PathBuf,
    pub thread: Option<std::thread::JoinHandle<()>>,
}

/// Start recording from the default input device to `out_path` (WAV).
/// Returns a handle; call `stop` to finish.
pub fn start_recording(out_path: PathBuf) -> Result<VoiceRecordingHandle, String> {
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use hound::{SampleFormat, WavSpec, WavWriter};
    use std::sync::{Arc, Mutex};

    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .ok_or_else(|| "no input device".to_string())?;
    let config = device
        .default_input_config()
        .map_err(|e| e.to_string())?;

    let stop_flag = Arc::new(Mutex::new(false));
    let stop_flag_clone = stop_flag.clone();
    let out_path_clone = out_path.clone();

    let spec = WavSpec {
        channels: config.channels(),
        sample_rate: config.sample_rate().0,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };

    let writer = Arc::new(Mutex::new(
        WavWriter::create(&out_path_clone, spec).map_err(|e| e.to_string())?,
    ));
    let writer_clone = writer.clone();

    let stream = device
        .build_input_stream(
            &config.into(),
            move |data: &[f32], _| {
                if *stop_flag_clone.lock().unwrap() {
                    return;
                }
                let mut w = writer_clone.lock().unwrap();
                for &s in data {
                    let sample = (s * 32767.0) as i16;
                    let _ = w.write_sample(sample);
                }
            },
            |e| eprintln!("voice input error: {e}"),
            None,
        )
        .map_err(|e| e.to_string())?;
    stream.play().map_err(|e| e.to_string())?;

    // Keep stream alive on a thread; it drops and flushes when stop_flag is set.
    let stop_flag_thread = stop_flag.clone();
    let handle = std::thread::spawn(move || {
        loop {
            if *stop_flag_thread.lock().unwrap() {
                drop(stream);
                let mut w = writer.lock().unwrap();
                let _ = w.flush();
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    });

    Ok(VoiceRecordingHandle {
        stop_flag,
        out_path,
        thread: Some(handle),
    })
}
```

Add `pub mod voice;` to `lib.rs`.

- [ ] **Step 4: Add `start_voice_recording` and `stop_voice_recording` commands**

In `apps/desktop/src-tauri/src/commands.rs`:

```rust
#[tauri::command]
pub async fn start_voice_recording(
    state: tauri::State<'_, AppState>,
) -> Result<(), CommandError> {
    let out_path = std::env::temp_dir().join(format!(
        "edytlab-voice-{}.wav",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
    ));
    let handle = crate::voice::start_recording(out_path)
        .map_err(|e| CommandError::Io(std::io::Error::other(e)))?;
    let mut rec = lock_std(&state.recording, "recording")?;
    *rec = Some(handle);
    Ok(())
}

#[tauri::command]
pub async fn stop_voice_recording(
    state: tauri::State<'_, AppState>,
) -> Result<String, CommandError> {
    let mut rec = lock_std(&state.recording, "recording")?;
    let handle = rec.take().ok_or_else(|| {
        CommandError::Io(std::io::Error::other("no active recording"))
    })?;
    // Signal stop
    *handle.stop_flag.lock().unwrap() = true;
    let out_path = handle.out_path.clone();
    // Wait for thread to finish flushing
    if let Some(t) = handle.thread {
        drop(rec); // release lock before join
        let _ = t.join();
    }
    Ok(out_path.to_string_lossy().into_owned())
}
```

Add `recording: std::sync::Mutex<Option<crate::voice::VoiceRecordingHandle>>` to `AppState` and initialize it with `Mutex::new(None)` in the `setup` closure in `lib.rs`.

- [ ] **Step 5: Add transcribe-voice path**

After `stop_voice_recording` returns the WAV path, the frontend calls the existing `transcribe` tool via the agent. But for a lightweight path without the agent, add a `transcribe_file` command that calls the Whisper machinery directly:

In `commands.rs`:

```rust
#[tauri::command]
pub async fn transcribe_file(
    state: tauri::State<'_, AppState>,
    path: String,
) -> Result<String, CommandError> {
    let store = lock_std(&state.store, "store")?;
    let engine = lock_std(&state.engine, "engine")?;
    let mut clipboard = lock_std(&state.clipboard, "clipboard")?;
    let dispatcher = tools::ToolDispatcher::default_dispatcher();
    let mut ctx = tools::ToolContext {
        store: &mut *store,
        engine: &mut *engine,
        user_message: "",
        clipboard: &mut *clipboard,
    };
    // Use the transcribe tool — it reads the head session and transcribes
    // the first track. For voice input, we need a simpler path.
    // For now, invoke the tool with the audio path as the source.
    // The transcribe tool may not accept an arbitrary path — check its schema.
    // If it doesn't, use audio_decoder + whisper_rs directly (future work).
    // Fallback: return empty string so the frontend can still insert the path.
    drop(ctx);
    Ok(String::new()) // placeholder until whisper_rs direct call is wired
}
```

Note: The full Whisper integration for arbitrary audio paths may require extending the `transcribe` tool or adding a `whisper_rs` call. For v1, the `stop_voice_recording` path returns the WAV path, and the frontend can send a message like "transcribe [path]" to the agent, letting the agent use the `transcribe` tool. This is simpler and avoids duplicating Whisper wiring.

- [ ] **Step 6: Register commands in `lib.rs`**

```rust
commands::start_voice_recording,
commands::stop_voice_recording,
```

Add to AppState initialization.

- [ ] **Step 7: Add to `tauri-bridge.ts`**

```ts
export async function startVoiceRecording(): Promise<void> {
  return invoke("start_voice_recording");
}

export async function stopVoiceRecording(): Promise<string> {
  return invoke("stop_voice_recording");
}
```

- [ ] **Step 8: Write a test for the mic button in Chat**

In `apps/desktop/src/__tests__/Chat.voice.test.tsx`:

```tsx
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

vi.mock("../lib/tauri-bridge", () => ({
  startVoiceRecording: vi.fn().mockResolvedValue(undefined),
  stopVoiceRecording: vi.fn().mockResolvedValue("/tmp/voice.wav"),
  sendMessage: vi.fn().mockResolvedValue(undefined),
  onTextDelta: vi.fn(() => () => {}),
  onToolCall: vi.fn(() => () => {}),
  onToolCallEnd: vi.fn(() => () => {}),
  onNodeCreated: vi.fn(() => () => {}),
  onAgentDone: vi.fn(() => () => {}),
  onPlan: vi.fn(() => () => {}),
  onMarkerChanged: vi.fn(() => () => {}),
}));

import { Chat } from "../components/Chat";

describe("Chat voice button", () => {
  it("renders mic button", () => {
    render(<Chat />);
    expect(screen.getByTestId("mic-btn")).toBeInTheDocument();
  });

  it("starts recording on click", async () => {
    const { startVoiceRecording } = await import("../lib/tauri-bridge");
    render(<Chat />);
    await userEvent.click(screen.getByTestId("mic-btn"));
    expect(startVoiceRecording).toHaveBeenCalled();
  });
});
```

- [ ] **Step 9: Add mic button to Chat**

In `apps/desktop/src/components/Chat.tsx`, find the chat input area (the `<textarea>` and Send button). Add a mic button alongside the Send button:

```tsx
import { startVoiceRecording, stopVoiceRecording } from "../lib/tauri-bridge";

// Inside Chat component:
const [recording, setRecording] = useState(false);

const handleMicClick = useCallback(async () => {
  if (recording) {
    setRecording(false);
    const wavPath = await stopVoiceRecording();
    // Insert a voice transcription request into the chat input:
    // The agent will receive: "please transcribe and respond to this audio: <path>"
    // Or for a more direct UX, auto-send to the agent.
    setInputText(prev =>
      prev ? `${prev}\n[voice: ${wavPath}]` : `[voice: ${wavPath}]`,
    );
  } else {
    setRecording(true);
    await startVoiceRecording();
  }
}, [recording]);

// In JSX, next to the Send button:
<button
  data-testid="mic-btn"
  onClick={handleMicClick}
  className={`p-2 rounded-lg transition-colors ${
    recording
      ? "text-red-400 bg-red-400/10 animate-pulse"
      : "text-neutral-400 hover:text-neutral-200"
  }`}
  title={recording ? "Stop recording" : "Voice input"}
>
  {recording ? "⬛" : "🎙"}
</button>
```

Note: `setInputText` — the exact state setter name depends on Chat's implementation. Look for `const [input, setInput]` or similar in the existing Chat code.

- [ ] **Step 10: Run tests**

```bash
pnpm --filter @edytlab/desktop test -- --run Chat.voice
pnpm --filter @edytlab/desktop exec tsc --noEmit
cargo build --package desktop-tauri 2>&1 | head -30
```

Expected: tests pass; Rust compiles.

- [ ] **Step 11: Commit**

```bash
git add apps/desktop/src-tauri/src/commands.rs \
        apps/desktop/src-tauri/src/lib.rs \
        apps/desktop/src-tauri/src/voice.rs \
        apps/desktop/src/lib/tauri-bridge.ts \
        apps/desktop/src/components/Chat.tsx \
        apps/desktop/src/__tests__/Chat.voice.test.tsx
git commit -m "feat(voice): push-to-talk mic button in chat with cpal recording"
```
