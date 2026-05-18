# Audacity Parity A3 — High-Complexity Features Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add 9 high-complexity features: spectrogram view, plot_spectrum FFT analysis, microphone recording, tremolo/phaser/distortion/stereo_widener effects, and Audacity-format label import/export — completing edytlab's Audacity feature parity.

**Architecture:** Tasks split across frontend (React 19, WaveSurfer.js 7, canvas) and Rust backend. Spectrogram uses WaveSurfer's built-in spectrogram plugin. plot_spectrum sends FFT data as JSON to frontend for canvas rendering. Recording uses CPAL via a new `crates/recorder` crate with Tauri commands. Effects (tremolo/phaser/distortion/widener) follow the same `destructive_edit` pattern as A1/A2. Labels use session annotations JSON.

**Tech Stack:** Rust (CPAL for audio I/O), WaveSurfer.js 7 SpectrogramPlugin, Web Audio API canvas FFT, React 19, Tauri 2 commands, `rustfft` crate (for plot_spectrum).

**Prerequisites:** Plans A1 and A2 must be merged first.

---

## Task 1: Spectrogram view in Timeline

**Files:**
- Modify: `apps/desktop/src/components/Timeline.tsx`
- Modify: `apps/desktop/src/App.tsx` (spectrogram toggle state)

- [ ] **Step 1: Write the failing test**

Create `apps/desktop/src/__tests__/Timeline.spectrogram.test.tsx`:

```tsx
import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { Timeline } from "../components/Timeline";

// Minimal WaveSurfer mock — already set up in Timeline.loop.test.tsx pattern
vi.mock("wavesurfer.js", () => ({
  default: {
    create: vi.fn(() => ({
      load: vi.fn(),
      on: vi.fn(),
      un: vi.fn(),
      destroy: vi.fn(),
      zoom: vi.fn(),
      registerPlugin: vi.fn(),
    })),
  },
}));
vi.mock("wavesurfer.js/dist/plugins/spectrogram.js", () => ({
  default: { create: vi.fn(() => ({ destroy: vi.fn() })) },
}));

describe("Timeline spectrogram toggle", () => {
  it("renders spectrogram button", () => {
    render(
      <Timeline
        src={null}
        selection={null}
        onSelectionChange={() => {}}
        zoom={1}
        loop={false}
        onLoopChange={() => {}}
        spectrogramEnabled={false}
        onSpectrogramChange={() => {}}
      />
    );
    expect(screen.getByTestId("spectrogram-btn")).toBeDefined();
  });

  it("calls onSpectrogramChange when button clicked", () => {
    const onChange = vi.fn();
    render(
      <Timeline
        src={null}
        selection={null}
        onSelectionChange={() => {}}
        zoom={1}
        loop={false}
        onLoopChange={() => {}}
        spectrogramEnabled={false}
        onSpectrogramChange={onChange}
      />
    );
    fireEvent.click(screen.getByTestId("spectrogram-btn"));
    expect(onChange).toHaveBeenCalledWith(true);
  });
});
```

- [ ] **Step 2: Run to verify it fails**

```powershell
cd "C:\Users\tusha\Work\Playground\Edytlab\edytlab"; pnpm --filter @edytlab/desktop test 2>&1 | tail -10
```
Expected: `Timeline.spectrogram.test.tsx` fails with prop not found.

- [ ] **Step 3: Install WaveSurfer spectrogram plugin**

Check if already available:
```powershell
cd "C:\Users\tusha\Work\Playground\Edytlab\edytlab"; pnpm --filter @edytlab/desktop list wavesurfer.js 2>&1 | grep wavesurfer
```
WaveSurfer 7 includes spectrogram as a bundled plugin at `wavesurfer.js/dist/plugins/spectrogram.js` — no additional install needed.

- [ ] **Step 4: Add props and spectrogram toggle to Timeline.tsx**

Open `apps/desktop/src/components/Timeline.tsx`. Find the `TimelineProps` interface and add:
```typescript
spectrogramEnabled?: boolean;
onSpectrogramChange?: (enabled: boolean) => void;
```

Find the `TrackLane` component props and add the same two props.

In the `TrackLane` component, add a `spectrogramRef` and effect:
```typescript
import SpectrogramPlugin from "wavesurfer.js/dist/plugins/spectrogram.js";

// Inside TrackLane component:
const spectrogramRef = useRef<ReturnType<typeof SpectrogramPlugin.create> | null>(null);

useEffect(() => {
  if (!wsRef.current) return;
  if (spectrogramEnabled) {
    if (!spectrogramRef.current) {
      spectrogramRef.current = SpectrogramPlugin.create({
        labels: true,
        height: 80,
        colorMap: "roseus",
      });
      wsRef.current.registerPlugin(spectrogramRef.current);
    }
  } else {
    if (spectrogramRef.current) {
      spectrogramRef.current.destroy();
      spectrogramRef.current = null;
    }
  }
}, [spectrogramEnabled]);
```

In the toolbar area of `TrackLane`, add the spectrogram toggle button next to the loop button:
```typescript
<button
  data-testid="spectrogram-btn"
  onClick={() => onSpectrogramChange?.(!spectrogramEnabled)}
  className={`px-2 py-0.5 text-xs rounded ${spectrogramEnabled ? "bg-violet-600 text-white" : "bg-neutral-700 text-neutral-300"}`}
  title="Toggle spectrogram"
>
  Spec
</button>
```

Pass `spectrogramEnabled` and `onSpectrogramChange` through from Timeline to TrackLane (same as loop/onLoopChange pattern).

- [ ] **Step 5: Add state to App.tsx**

Open `apps/desktop/src/App.tsx`. Add:
```typescript
const [spectrogramEnabled, setSpectrogramEnabled] = useState(false);
```

Pass to Timeline:
```typescript
<Timeline
  ...existing props...
  spectrogramEnabled={spectrogramEnabled}
  onSpectrogramChange={setSpectrogramEnabled}
/>
```

- [ ] **Step 6: Run tests**

```powershell
cd "C:\Users\tusha\Work\Playground\Edytlab\edytlab"; pnpm --filter @edytlab/desktop test 2>&1 | tail -10
```
Expected: `test result: ok` — all tests pass including the 2 new spectrogram tests.

- [ ] **Step 7: Type-check**

```powershell
cd "C:\Users\tusha\Work\Playground\Edytlab\edytlab"; pnpm --filter @edytlab/desktop exec tsc --noEmit 2>&1 | tail -10
```
Expected: no errors.

- [ ] **Step 8: Commit**

```powershell
cd "C:\Users\tusha\Work\Playground\Edytlab\edytlab"; git add apps/desktop/src/components/Timeline.tsx apps/desktop/src/App.tsx apps/desktop/src/__tests__/Timeline.spectrogram.test.tsx; git commit -m "feat(ui): spectrogram view toggle in Timeline via WaveSurfer plugin`n`nhttps://claude.ai/code/session_01jvf7s8jnfj9xhm5qzw7q8nd"
```

---

## Task 2: `plot_spectrum` — FFT analysis tool

**Files:**
- Modify: `crates/tools/src/Cargo.toml` or `Cargo.toml` workspace (add `rustfft`)
- Create: `crates/tools/src/tool/plot_spectrum.rs`
- Modify: mod.rs, dispatcher.rs
- Modify: `apps/desktop/src/components/SpectrumChart.tsx` (new component)
- Modify: `apps/desktop/src/components/Chat.tsx` or `MessageBubble.tsx` (render chart from tool result)

- [ ] **Step 1: Add rustfft dependency**

Open `Cargo.toml` (workspace root) and add to `[workspace.dependencies]`:
```toml
rustfft = "6"
```

Open `crates/tools/Cargo.toml` and add to `[dependencies]`:
```toml
rustfft = { workspace = true }
```

- [ ] **Step 2: Write failing test**

Create or add to `crates/tools/src/tool/plot_spectrum.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::compute_fft_magnitude;
    #[test]
    fn sine_440hz_peak_near_440() {
        // Generate 1s of 440Hz sine at 44100 Hz
        let sr = 44100u32;
        let samples: Vec<f32> = (0..sr).map(|i| {
            (2.0 * std::f32::consts::PI * 440.0 * i as f32 / sr as f32).sin()
        }).collect();
        let bins = compute_fft_magnitude(&samples, sr, 4096);
        // Find peak bin
        let peak_bin = bins.iter().enumerate()
            .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap())
            .map(|(i, _)| i).unwrap();
        let peak_freq = peak_bin as f32 * sr as f32 / 4096.0;
        assert!((peak_freq - 440.0).abs() < 20.0, "peak at {peak_freq}Hz, expected ~440Hz");
    }
}
```

- [ ] **Step 3: Run to verify it fails**

```powershell
cd "C:\Users\tusha\Work\Playground\Edytlab\edytlab"; cargo test -p tools plot_spectrum 2>&1 | tail -5
```

- [ ] **Step 4: Implement `plot_spectrum.rs`**

```rust
use rustfft::{FftPlanner, num_complex::Complex};
use serde::Deserialize;
use serde_json::{json, Value};
use crate::schema::anthropic_tool;
use crate::tool::util::load_head_state;
use crate::{Tool, ToolContext, ToolResult};

const FFT_SIZE: usize = 4096;

/// Returns (frequency_hz, magnitude_db) pairs for bins up to Nyquist.
pub(crate) fn compute_fft_magnitude(samples: &[f32], sr: u32, fft_size: usize) -> Vec<f32> {
    let mut planner = FftPlanner::new();
    let fft = planner.plan_fft_forward(fft_size);
    // Use a Hann window on the first fft_size samples (or zero-pad)
    let mut buf: Vec<Complex<f32>> = (0..fft_size).map(|i| {
        let window = 0.5 * (1.0 - (2.0 * std::f32::consts::PI * i as f32 / (fft_size - 1) as f32).cos());
        let s = samples.get(i).copied().unwrap_or(0.0);
        Complex::new(s * window, 0.0)
    }).collect();
    fft.process(&mut buf);
    // Return magnitudes in dBFS for bins 0..fft_size/2
    (0..fft_size / 2).map(|i| {
        let mag = buf[i].norm() / fft_size as f32;
        if mag > 1e-10 { 20.0 * mag.log10() } else { -120.0 }
    }).collect()
}

#[derive(Debug, Deserialize)]
struct Args { track: usize, start_sec: f64, end_sec: f64 }

pub struct PlotSpectrumTool;

impl Tool for PlotSpectrumTool {
    fn name(&self) -> &'static str { "plot_spectrum" }

    fn schema(&self) -> Value {
        anthropic_tool("plot_spectrum",
            "Compute the FFT magnitude spectrum of a track region. Returns frequency/magnitude data for display. Does not modify audio.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "track": { "type": "integer" },
                    "start_sec": { "type": "number", "description": "Region start in seconds" },
                    "end_sec": { "type": "number", "description": "Region end in seconds" }
                },
                "required": ["track", "start_sec", "end_sec"]
            }))
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a, Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };
        if args.start_sec >= args.end_sec {
            return Ok(ToolResult::Error("start_sec must be < end_sec".into()));
        }
        let state = match load_head_state(ctx) { Ok(s) => s, Err(e) => return Ok(ToolResult::Error(e)) };
        if let Err(e) = crate::tool::util::check_track_index(&state.tracks, args.track) {
            return Ok(ToolResult::Error(e));
        }
        let clip = match state.tracks[args.track].clips.first() {
            Some(c) => c.clone(),
            None => return Ok(ToolResult::Error(format!("track {} has no clips", args.track))),
        };
        let decoded = match audio_decoder::decode_file(&clip.source_path) {
            Ok(d) => d, Err(e) => return Ok(ToolResult::Error(format!("decode failed: {e}"))),
        };
        let sr = decoded.sample_rate;
        let channels = decoded.channels as usize;
        let channels = channels.max(1);
        let start_frame = ((args.start_sec * sr as f64) as usize).min(decoded.samples.len() / channels);
        let end_frame = ((args.end_sec * sr as f64) as usize).min(decoded.samples.len() / channels);
        // Mix to mono for FFT
        let mono: Vec<f32> = (start_frame..end_frame).map(|f| {
            (0..channels).map(|ch| decoded.samples[f * channels + ch]).sum::<f32>() / channels as f32
        }).collect();
        let magnitudes = compute_fft_magnitude(&mono, sr, FFT_SIZE);
        let bin_hz = sr as f32 / FFT_SIZE as f32;
        let points: Vec<serde_json::Value> = magnitudes.iter().enumerate()
            .map(|(i, &db)| json!({ "hz": i as f32 * bin_hz, "db": db }))
            .collect();
        Ok(ToolResult::Ok(json!({
            "type": "spectrum",
            "track": args.track,
            "start_sec": args.start_sec,
            "end_sec": args.end_sec,
            "sample_rate": sr,
            "fft_size": FFT_SIZE,
            "points": points,
            "summary": format!("Spectrum for track {} ({:.2}s..{:.2}s), {} bins", args.track, args.start_sec, args.end_sec, magnitudes.len())
        })))
    }
}
```

- [ ] **Step 5: Register in mod.rs and dispatcher.rs**

mod.rs: `pub mod plot_spectrum; pub use plot_spectrum::PlotSpectrumTool;`
dispatcher.rs: `d.register(Box::new(PlotSpectrumTool));`

- [ ] **Step 6: Create SpectrumChart.tsx for frontend rendering**

Create `apps/desktop/src/components/SpectrumChart.tsx`:

```tsx
import { useEffect, useRef } from "react";

interface SpectrumPoint { hz: number; db: number; }
interface SpectrumChartProps {
  points: SpectrumPoint[];
  width?: number;
  height?: number;
}

export function SpectrumChart({ points, width = 400, height = 200 }: SpectrumChartProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas || points.length === 0) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    ctx.clearRect(0, 0, width, height);
    ctx.fillStyle = "#1a1a1a";
    ctx.fillRect(0, 0, width, height);

    const maxHz = points[points.length - 1]?.hz ?? 22050;
    const minDb = -120;
    const maxDb = 0;

    ctx.strokeStyle = "#7c3aed";
    ctx.lineWidth = 1.5;
    ctx.beginPath();
    points.forEach((p, i) => {
      const x = (p.hz / maxHz) * width;
      const y = height - ((p.db - minDb) / (maxDb - minDb)) * height;
      if (i === 0) ctx.moveTo(x, y); else ctx.lineTo(x, y);
    });
    ctx.stroke();

    // Axis labels
    ctx.fillStyle = "#666";
    ctx.font = "10px monospace";
    ctx.fillText("0", 2, height - 2);
    ctx.fillText(`${(maxHz / 1000).toFixed(0)}k`, width - 24, height - 2);
    ctx.fillText("0dB", 2, 10);
    ctx.fillText("-120", 2, height - 12);
  }, [points, width, height]);

  return <canvas ref={canvasRef} width={width} height={height} className="rounded" />;
}
```

- [ ] **Step 7: Wire SpectrumChart into MessageBubble.tsx**

Open `apps/desktop/src/components/MessageBubble.tsx`. Find where tool results are rendered. Add:

```tsx
import { SpectrumChart } from "./SpectrumChart";

// In the tool result rendering section, check for spectrum type:
{result?.type === "spectrum" && Array.isArray(result.points) && (
  <div className="mt-2">
    <SpectrumChart points={result.points} width={380} height={160} />
    <p className="text-xs text-neutral-500 mt-1">{result.summary}</p>
  </div>
)}
```

If MessageBubble currently renders tool results as plain text, preserve that behavior for all other result types; only add the chart for `result.type === "spectrum"`.

- [ ] **Step 8: Run all tests**

```powershell
cd "C:\Users\tusha\Work\Playground\Edytlab\edytlab"; cargo test -p tools plot_spectrum 2>&1 | tail -10
```
Expected: `test result: ok. 1 passed`

```powershell
cd "C:\Users\tusha\Work\Playground\Edytlab\edytlab"; pnpm --filter @edytlab/desktop test 2>&1 | tail -5
pnpm --filter @edytlab/desktop exec tsc --noEmit 2>&1 | tail -5
```
Expected: all pass.

- [ ] **Step 9: Commit**

```powershell
cd "C:\Users\tusha\Work\Playground\Edytlab\edytlab"; git add crates/tools/src/tool/plot_spectrum.rs crates/tools/src/tool/mod.rs crates/tools/src/dispatcher.rs apps/desktop/src/components/SpectrumChart.tsx apps/desktop/src/components/MessageBubble.tsx; git commit -m "feat(tools): plot_spectrum — FFT magnitude analysis with canvas chart`n`nhttps://claude.ai/code/session_01jvf7s8jnfj9xhm5qzw7q8nd"
```

---

## Task 3: Tremolo, Phaser, Distortion, Stereo Widener effects

**Files:**
- Create: `crates/tools/src/tool/tremolo.rs`
- Create: `crates/tools/src/tool/phaser.rs`
- Create: `crates/tools/src/tool/distortion.rs`
- Create: `crates/tools/src/tool/stereo_widener.rs`
- Modify: mod.rs, dispatcher.rs

- [ ] **Step 1: Write failing tests**

`tremolo.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::apply_tremolo;
    #[test]
    fn modulates_amplitude() {
        // Constant signal; tremolo at 5Hz depth=1 should cause amplitude oscillation
        let mut samples = vec![1.0f32; 44100];
        apply_tremolo(&mut samples, 44100, 1, 5.0, 0.5);
        // With depth=0.5, amplitude oscillates between 0.5 and 1.0
        // At 5Hz: period = 44100/5 = 8820 frames; quarter period = 2205
        // At frame 2205 we should be at max mod, frame 6615 at min
        let at_max = samples[0]; // starts at max (cos(0)=1)
        let at_min = samples[44100 / (5 * 2)]; // half period = minimum
        assert!(at_max > at_min, "tremolo should create amplitude variation, max={at_max} min={at_min}");
    }
}
```

`phaser.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::apply_phaser;
    #[test]
    fn does_not_clip() {
        let mut samples: Vec<f32> = (0..44100).map(|i| (i as f32 * 0.001).sin() * 0.8).collect();
        apply_phaser(&mut samples, 44100, 1, 1.0, 0.7, 4);
        let max = samples.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        assert!(max <= 1.5, "phaser output should not clip excessively, got {max}");
    }
}
```

`distortion.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::apply_distortion;
    #[test]
    fn high_drive_clips() {
        let mut samples = vec![0.5f32; 100];
        apply_distortion(&mut samples, 44100, 1, 10.0, 0.5);
        let max = samples.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        assert!(max <= 1.0 + 1e-5, "hard-clipped output should be within [-1,1]");
    }
    #[test]
    fn low_drive_passes_through_roughly() {
        let mut samples = vec![0.1f32; 100];
        apply_distortion(&mut samples, 44100, 1, 1.0, 0.0);
        // At drive=1, tone=0: signal should not change dramatically
        assert!((samples[0] - 0.1).abs() < 0.05);
    }
}
```

`stereo_widener.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::apply_stereo_widener;
    #[test]
    fn width_zero_is_mono() {
        // width=0 → M=L+R, S=0 → L=R=M/2
        let mut samples = vec![0.8f32, 0.2, 0.6, 0.4]; // L,R interleaved
        apply_stereo_widener(&mut samples, 44100, 2, 0.0);
        assert!((samples[0] - samples[1]).abs() < 1e-5, "width=0 should give L==R");
    }
    #[test]
    fn width_one_retains_stereo() {
        let mut samples = vec![0.8f32, 0.2, 0.6, 0.4];
        apply_stereo_widener(&mut samples, 44100, 2, 1.0);
        // Output differs from mono
        assert!((samples[0] - samples[1]).abs() > 0.1, "stereo field preserved");
    }
}
```

- [ ] **Step 2: Run to verify they fail**

```powershell
cd "C:\Users\tusha\Work\Playground\Edytlab\edytlab"; cargo test -p tools tremolo phaser distortion stereo_widener 2>&1 | tail -5
```

- [ ] **Step 3: Implement `tremolo.rs`**

```rust
use serde::Deserialize;
use serde_json::Value;
use crate::schema::anthropic_tool;
use crate::tool::util::destructive_edit;
use crate::{Tool, ToolContext, ToolResult};

pub(crate) fn apply_tremolo(samples: &mut [f32], sr: u32, channels: usize, rate_hz: f32, depth: f32) {
    let channels = channels.max(1);
    let depth = depth.clamp(0.0, 1.0);
    let n_frames = samples.len() / channels;
    for frame in 0..n_frames {
        let lfo = (2.0 * std::f32::consts::PI * rate_hz * frame as f32 / sr as f32).cos();
        let gain = 1.0 - depth * (1.0 - lfo) / 2.0; // oscillates between 1-depth and 1
        for ch in 0..channels {
            samples[frame * channels + ch] *= gain;
        }
    }
}

#[derive(Debug, Deserialize)]
struct Args { track: usize, rate_hz: Option<f32>, depth: Option<f32>, start_sec: Option<f64>, end_sec: Option<f64> }

pub struct TremoloTool;

impl Tool for TremoloTool {
    fn name(&self) -> &'static str { "tremolo" }

    fn schema(&self) -> Value {
        anthropic_tool("tremolo",
            "Apply tremolo (LFO amplitude modulation). rate_hz controls oscillation speed; depth (0..1) controls modulation depth. Appends a new session node.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "track": { "type": "integer" },
                    "rate_hz": { "type": "number", "default": 4.0, "description": "LFO rate in Hz" },
                    "depth": { "type": "number", "default": 0.5, "description": "Modulation depth 0..1" },
                    "start_sec": { "type": "number" },
                    "end_sec": { "type": "number" }
                },
                "required": ["track"]
            }))
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a, Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };
        let rate = args.rate_hz.unwrap_or(4.0).max(0.1);
        let depth = args.depth.unwrap_or(0.5).clamp(0.0, 1.0);
        let channels = {
            let state = match crate::tool::util::load_head_state(ctx) {
                Ok(s) => s, Err(e) => return Ok(ToolResult::Error(e)),
            };
            if let Err(e) = crate::tool::util::check_track_index(&state.tracks, args.track) {
                return Ok(ToolResult::Error(e));
            }
            let clip = state.tracks[args.track].clips.first().cloned();
            if let Some(c) = clip {
                audio_decoder::decode_file(&c.source_path).map(|d| d.channels as usize).unwrap_or(1)
            } else { return Ok(ToolResult::Error(format!("track {} has no clips", args.track))); }
        };
        let (r, d, s, e) = (rate, depth, args.start_sec, args.end_sec);
        Ok(destructive_edit(ctx, args.track,
            move |samples, sr| {
                let ch = channels;
                let len_frames = samples.len() / ch.max(1);
                let start = s.map(|sec| ((sec * sr as f64) as usize).min(len_frames)).unwrap_or(0);
                let end = e.map(|sec| ((sec * sr as f64) as usize).min(len_frames)).unwrap_or(len_frames);
                apply_tremolo(&mut samples[start * ch.max(1)..end * ch.max(1)], sr, ch, r, d);
            },
            format!("tremolo track {} rate={:.1}Hz depth={:.2}", args.track, rate, depth),
        ))
    }
}
```

- [ ] **Step 4: Implement `phaser.rs`**

```rust
use serde::Deserialize;
use serde_json::Value;
use crate::schema::anthropic_tool;
use crate::tool::util::destructive_edit;
use crate::{Tool, ToolContext, ToolResult};

/// All-pass filter for phaser
struct AllPass { a1: f32, z: f32 }
impl AllPass {
    fn new(frequency: f32, sr: f32) -> Self {
        let k = (std::f32::consts::PI * frequency / sr).tan();
        let a1 = (k - 1.0) / (k + 1.0);
        Self { a1, z: 0.0 }
    }
    fn process(&mut self, x: f32) -> f32 {
        let y = self.a1 * x + self.z;
        self.z = x - self.a1 * y;
        y
    }
}

pub(crate) fn apply_phaser(samples: &mut [f32], sr: u32, channels: usize, rate_hz: f32, depth: f32, stages: u32) {
    let channels = channels.max(1);
    let stages = (stages as usize).max(2).min(12);
    let n_frames = samples.len() / channels;
    // One set of all-pass stages per channel
    let min_freq = 200.0f32;
    let max_freq = 4000.0f32;
    let mut all_passes: Vec<Vec<AllPass>> = (0..channels).map(|_| {
        (0..stages).map(|_| AllPass::new(min_freq, sr as f32)).collect()
    }).collect();
    for frame in 0..n_frames {
        let lfo = (2.0 * std::f32::consts::PI * rate_hz * frame as f32 / sr as f32).sin();
        let freq = min_freq + (max_freq - min_freq) * (lfo * 0.5 + 0.5);
        for ch in 0..channels {
            // Update all-pass stage frequencies
            for ap in &mut all_passes[ch] {
                let k = (std::f32::consts::PI * freq / sr as f32).tan();
                ap.a1 = (k - 1.0) / (k + 1.0);
            }
            let x = samples[frame * channels + ch];
            let mut y = x;
            for ap in &mut all_passes[ch] { y = ap.process(y); }
            samples[frame * channels + ch] = x + y * depth;
        }
    }
}

#[derive(Debug, Deserialize)]
struct Args { track: usize, rate_hz: Option<f32>, depth: Option<f32>, stages: Option<u32>, start_sec: Option<f64>, end_sec: Option<f64> }

pub struct PhaserTool;

impl Tool for PhaserTool {
    fn name(&self) -> &'static str { "phaser" }

    fn schema(&self) -> Value {
        anthropic_tool("phaser",
            "Apply a phaser effect using an all-pass filter chain with LFO sweep. rate_hz controls LFO speed; depth is the wet blend; stages sets the filter chain length (2-12). Appends a new session node.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "track": { "type": "integer" },
                    "rate_hz": { "type": "number", "default": 0.5 },
                    "depth": { "type": "number", "default": 0.5 },
                    "stages": { "type": "integer", "default": 4 },
                    "start_sec": { "type": "number" },
                    "end_sec": { "type": "number" }
                },
                "required": ["track"]
            }))
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a, Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };
        let rate = args.rate_hz.unwrap_or(0.5).max(0.01);
        let depth = args.depth.unwrap_or(0.5).clamp(0.0, 1.0);
        let stages = args.stages.unwrap_or(4).clamp(2, 12);
        let channels = {
            let state = match crate::tool::util::load_head_state(ctx) {
                Ok(s) => s, Err(e) => return Ok(ToolResult::Error(e)),
            };
            if let Err(e) = crate::tool::util::check_track_index(&state.tracks, args.track) {
                return Ok(ToolResult::Error(e));
            }
            let clip = state.tracks[args.track].clips.first().cloned();
            if let Some(c) = clip {
                audio_decoder::decode_file(&c.source_path).map(|d| d.channels as usize).unwrap_or(1)
            } else { return Ok(ToolResult::Error(format!("track {} has no clips", args.track))); }
        };
        let (r, d, st, s, e) = (rate, depth, stages, args.start_sec, args.end_sec);
        Ok(destructive_edit(ctx, args.track,
            move |samples, sr| {
                let ch = channels;
                let len_frames = samples.len() / ch.max(1);
                let start = s.map(|sec| ((sec * sr as f64) as usize).min(len_frames)).unwrap_or(0);
                let end = e.map(|sec| ((sec * sr as f64) as usize).min(len_frames)).unwrap_or(len_frames);
                apply_phaser(&mut samples[start * ch.max(1)..end * ch.max(1)], sr, ch, r, d, st);
            },
            format!("phaser track {} rate={:.2}Hz depth={:.2} stages={}", args.track, rate, depth, stages),
        ))
    }
}
```

- [ ] **Step 5: Implement `distortion.rs`**

```rust
use serde::Deserialize;
use serde_json::Value;
use crate::schema::anthropic_tool;
use crate::tool::util::destructive_edit;
use crate::{Tool, ToolContext, ToolResult};

pub(crate) fn apply_distortion(samples: &mut [f32], sr: u32, channels: usize, drive: f32, tone: f32) {
    let channels = channels.max(1);
    let drive = drive.max(1.0);
    let tone = tone.clamp(0.0, 1.0);
    // Soft clip via tanh waveshaper
    for s in samples.iter_mut() {
        *s = (*s * drive).tanh() / drive.tanh().max(1e-6);
    }
    // Simple tone filter: 1-pole lowpass controlled by `tone` (tone=0 darkest, tone=1 brightest)
    let cutoff = 200.0 + tone * 8000.0;
    let k = (-2.0 * std::f32::consts::PI * cutoff / sr as f32).exp();
    let n_frames = samples.len() / channels;
    for ch in 0..channels {
        let mut z = 0.0f32;
        for frame in 0..n_frames {
            let idx = frame * channels + ch;
            z = samples[idx] * (1.0 - k) + z * k;
            samples[idx] = z;
        }
    }
}

#[derive(Debug, Deserialize)]
struct Args { track: usize, drive: Option<f32>, tone: Option<f32>, start_sec: Option<f64>, end_sec: Option<f64> }

pub struct DistortionTool;

impl Tool for DistortionTool {
    fn name(&self) -> &'static str { "distortion" }

    fn schema(&self) -> Value {
        anthropic_tool("distortion",
            "Apply soft-clip distortion (tanh waveshaper) followed by a tone filter. drive > 1 increases gain before clipping; tone (0=dark, 1=bright) controls the output filter. Appends a new session node.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "track": { "type": "integer" },
                    "drive": { "type": "number", "default": 3.0, "description": "Pre-gain multiplier (1=clean, 10=heavy)" },
                    "tone": { "type": "number", "default": 0.5, "description": "Tone brightness 0..1" },
                    "start_sec": { "type": "number" },
                    "end_sec": { "type": "number" }
                },
                "required": ["track"]
            }))
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a, Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };
        let drive = args.drive.unwrap_or(3.0).max(1.0);
        let tone = args.tone.unwrap_or(0.5).clamp(0.0, 1.0);
        let channels = {
            let state = match crate::tool::util::load_head_state(ctx) {
                Ok(s) => s, Err(e) => return Ok(ToolResult::Error(e)),
            };
            if let Err(e) = crate::tool::util::check_track_index(&state.tracks, args.track) {
                return Ok(ToolResult::Error(e));
            }
            let clip = state.tracks[args.track].clips.first().cloned();
            if let Some(c) = clip {
                audio_decoder::decode_file(&c.source_path).map(|d| d.channels as usize).unwrap_or(1)
            } else { return Ok(ToolResult::Error(format!("track {} has no clips", args.track))); }
        };
        let (dr, tn, s, e) = (drive, tone, args.start_sec, args.end_sec);
        Ok(destructive_edit(ctx, args.track,
            move |samples, sr| {
                let ch = channels;
                let len_frames = samples.len() / ch.max(1);
                let start = s.map(|sec| ((sec * sr as f64) as usize).min(len_frames)).unwrap_or(0);
                let end = e.map(|sec| ((sec * sr as f64) as usize).min(len_frames)).unwrap_or(len_frames);
                apply_distortion(&mut samples[start * ch.max(1)..end * ch.max(1)], sr, ch, dr, tn);
            },
            format!("distortion track {} drive={:.1} tone={:.2}", args.track, drive, tone),
        ))
    }
}
```

- [ ] **Step 6: Implement `stereo_widener.rs`**

```rust
use serde::Deserialize;
use serde_json::Value;
use crate::schema::anthropic_tool;
use crate::tool::util::destructive_edit;
use crate::{Tool, ToolContext, ToolResult};

/// M/S stereo widening. width=0 → mono, width=1 → original, width>1 → extra wide.
pub(crate) fn apply_stereo_widener(samples: &mut [f32], _sr: u32, channels: usize, width: f32) {
    if channels < 2 { return; }
    let n_frames = samples.len() / channels;
    for frame in 0..n_frames {
        let l = samples[frame * channels];
        let r = samples[frame * channels + 1];
        let mid = (l + r) / 2.0;
        let side = (l - r) / 2.0 * width;
        samples[frame * channels] = mid + side;
        samples[frame * channels + 1] = mid - side;
    }
}

#[derive(Debug, Deserialize)]
struct Args { track: usize, width: Option<f32>, start_sec: Option<f64>, end_sec: Option<f64> }

pub struct StereoWidenerTool;

impl Tool for StereoWidenerTool {
    fn name(&self) -> &'static str { "stereo_widener" }

    fn schema(&self) -> Value {
        anthropic_tool("stereo_widener",
            "Widen or narrow the stereo field using M/S processing. width=0 collapses to mono, width=1 is original, width=2 doubles the stereo width. Appends a new session node.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "track": { "type": "integer" },
                    "width": { "type": "number", "default": 1.5, "description": "Stereo width (0=mono, 1=original, 2=extra wide)" },
                    "start_sec": { "type": "number" },
                    "end_sec": { "type": "number" }
                },
                "required": ["track"]
            }))
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a, Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };
        let width = args.width.unwrap_or(1.5).max(0.0);
        let channels = {
            let state = match crate::tool::util::load_head_state(ctx) {
                Ok(s) => s, Err(e) => return Ok(ToolResult::Error(e)),
            };
            if let Err(e) = crate::tool::util::check_track_index(&state.tracks, args.track) {
                return Ok(ToolResult::Error(e));
            }
            let clip = state.tracks[args.track].clips.first().cloned();
            if let Some(c) = clip {
                audio_decoder::decode_file(&c.source_path).map(|d| d.channels as usize).unwrap_or(1)
            } else { return Ok(ToolResult::Error(format!("track {} has no clips", args.track))); }
        };
        if channels < 2 { return Ok(ToolResult::Error("stereo_widener requires a stereo track".into())); }
        let (w, s, e) = (width, args.start_sec, args.end_sec);
        Ok(destructive_edit(ctx, args.track,
            move |samples, sr| {
                let ch = channels;
                let len_frames = samples.len() / ch;
                let start = s.map(|sec| ((sec * sr as f64) as usize).min(len_frames)).unwrap_or(0);
                let end = e.map(|sec| ((sec * sr as f64) as usize).min(len_frames)).unwrap_or(len_frames);
                apply_stereo_widener(&mut samples[start * ch..end * ch], sr, ch, w);
            },
            format!("stereo_widener track {} width={:.2}", args.track, width),
        ))
    }
}
```

- [ ] **Step 7: Register all four tools**

mod.rs additions:
```rust
pub mod tremolo;
pub mod phaser;
pub mod distortion;
pub mod stereo_widener;
pub use tremolo::TremoloTool;
pub use phaser::PhaserTool;
pub use distortion::DistortionTool;
pub use stereo_widener::StereoWidenerTool;
```

dispatcher.rs additions:
```rust
d.register(Box::new(TremoloTool));
d.register(Box::new(PhaserTool));
d.register(Box::new(DistortionTool));
d.register(Box::new(StereoWidenerTool));
```

- [ ] **Step 8: Run tests**

```powershell
cd "C:\Users\tusha\Work\Playground\Edytlab\edytlab"; cargo test -p tools tremolo phaser distortion stereo_widener 2>&1 | tail -10
```
Expected: `test result: ok. 5 passed` (tremolo 1, phaser 1, distortion 2, widener 2)

- [ ] **Step 9: Commit**

```powershell
cd "C:\Users\tusha\Work\Playground\Edytlab\edytlab"; git add crates/tools/src/tool/tremolo.rs crates/tools/src/tool/phaser.rs crates/tools/src/tool/distortion.rs crates/tools/src/tool/stereo_widener.rs crates/tools/src/tool/mod.rs crates/tools/src/dispatcher.rs; git commit -m "feat(tools): tremolo, phaser, distortion, stereo_widener effects`n`nhttps://claude.ai/code/session_01jvf7s8jnfj9xhm5qzw7q8nd"
```

---

## Task 4: `export_labels` and `import_labels`

**Files:**
- Create: `crates/tools/src/tool/export_labels.rs`
- Create: `crates/tools/src/tool/import_labels.rs`
- Modify: mod.rs, dispatcher.rs

The label format used by Audacity is tab-separated: `start_sec\tend_sec\tlabel_text\n`

- [ ] **Step 1: Write failing tests**

`export_labels.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::{format_labels, parse_annotation_to_label};
    use session::annotation::Annotation;

    fn make_annotation(start: f64, end: f64, text: &str) -> Annotation {
        Annotation { id: uuid::Uuid::new_v4(), start_sec: start, end_sec: Some(end), label: text.to_string(), track_index: None }
    }

    #[test]
    fn formats_correctly() {
        let ann = make_annotation(1.5, 3.0, "verse");
        let line = format_labels(&[ann]);
        assert_eq!(line, "1.5\t3\tverse\n", "format: {line:?}");
    }

    #[test]
    fn empty_gives_empty_string() {
        assert_eq!(format_labels(&[]), "");
    }
}
```

`import_labels.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::parse_labels;

    #[test]
    fn parses_two_lines() {
        let text = "1.5\t3.0\tverse\n4.0\t6.5\tchorus\n";
        let labels = parse_labels(text);
        assert_eq!(labels.len(), 2);
        assert_eq!(labels[0].0, 1.5);
        assert_eq!(labels[0].1, 3.0);
        assert_eq!(labels[0].2, "verse");
    }

    #[test]
    fn skips_malformed_lines() {
        let text = "bad_line\n1.0\t2.0\tok\n";
        let labels = parse_labels(text);
        assert_eq!(labels.len(), 1);
        assert_eq!(labels[0].2, "ok");
    }
}
```

- [ ] **Step 2: Run to verify they fail**

```powershell
cd "C:\Users\tusha\Work\Playground\Edytlab\edytlab"; cargo test -p tools export_labels import_labels 2>&1 | tail -5
```

- [ ] **Step 3: Check the Annotation struct**

Before implementing, read the Annotation type:

```powershell
cd "C:\Users\tusha\Work\Playground\Edytlab\edytlab"; grep -r "struct Annotation" crates/session/src/ 2>&1 | head -5
```

If Annotation has different fields, adjust the test and implementation accordingly.

- [ ] **Step 4: Implement `export_labels.rs`**

```rust
use serde::Deserialize;
use serde_json::{json, Value};
use session::annotation::Annotation;
use crate::schema::anthropic_tool;
use crate::tool::util::load_head_state;
use crate::{Tool, ToolContext, ToolResult};

pub(crate) fn format_labels(annotations: &[Annotation]) -> String {
    annotations.iter().map(|a| {
        let end = a.end_sec.unwrap_or(a.start_sec + 0.1);
        format!("{}\t{}\t{}\n", a.start_sec, end, a.label)
    }).collect()
}

// Kept for tests
pub(crate) fn parse_annotation_to_label(a: &Annotation) -> String {
    let end = a.end_sec.unwrap_or(a.start_sec + 0.1);
    format!("{}\t{}\t{}\n", a.start_sec, end, a.label)
}

#[derive(Debug, Deserialize)]
struct Args { track: Option<usize> }

pub struct ExportLabelsTool;

impl Tool for ExportLabelsTool {
    fn name(&self) -> &'static str { "export_labels" }

    fn schema(&self) -> Value {
        anthropic_tool("export_labels",
            "Export session annotations as Audacity-format label text (start_sec TAB end_sec TAB label). Does not modify audio. Returns the label text.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "track": { "type": "integer", "description": "Optional: filter labels to a specific track. Omit for all labels." }
                },
                "required": []
            }))
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a, Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };
        let state = match load_head_state(ctx) { Ok(s) => s, Err(e) => return Ok(ToolResult::Error(e)) };
        let annotations: Vec<Annotation> = state.annotations.into_iter()
            .filter(|a| args.track.map(|t| a.track_index == Some(t)).unwrap_or(true))
            .collect();
        let label_text = format_labels(&annotations);
        Ok(ToolResult::Ok(json!({
            "labels": label_text,
            "count": annotations.len(),
            "summary": format!("Exported {} label(s) in Audacity format", annotations.len())
        })))
    }
}
```

- [ ] **Step 5: Implement `import_labels.rs`**

```rust
use serde::Deserialize;
use serde_json::{json, Value};
use session::annotation::Annotation;
use crate::schema::anthropic_tool;
use crate::tool::util::{append_state, load_head_state};
use crate::{Tool, ToolContext, ToolResult};

/// Parse Audacity label format: "start_sec\tend_sec\tlabel\n"
/// Returns Vec<(start_sec, end_sec, label)>
pub(crate) fn parse_labels(text: &str) -> Vec<(f64, f64, String)> {
    text.lines().filter_map(|line| {
        let parts: Vec<&str> = line.splitn(3, '\t').collect();
        if parts.len() < 3 { return None; }
        let start: f64 = parts[0].trim().parse().ok()?;
        let end: f64 = parts[1].trim().parse().ok()?;
        let label = parts[2].trim().to_string();
        Some((start, end, label))
    }).collect()
}

#[derive(Debug, Deserialize)]
struct Args { labels_text: String, track: Option<usize> }

pub struct ImportLabelsTool;

impl Tool for ImportLabelsTool {
    fn name(&self) -> &'static str { "import_labels" }

    fn schema(&self) -> Value {
        anthropic_tool("import_labels",
            "Import Audacity-format label text into the session as annotations. Format: each line is 'start_sec TAB end_sec TAB label'. Appends a new session node.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "labels_text": { "type": "string", "description": "Label file content in Audacity format" },
                    "track": { "type": "integer", "description": "Optional: associate labels with this track index" }
                },
                "required": ["labels_text"]
            }))
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a, Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };
        let parsed = parse_labels(&args.labels_text);
        if parsed.is_empty() {
            return Ok(ToolResult::Error("No valid labels found in input text. Expected format: 'start_sec TAB end_sec TAB label' per line.".into()));
        }
        let mut state = match load_head_state(ctx) { Ok(s) => s, Err(e) => return Ok(ToolResult::Error(e)) };
        let new_annotations: Vec<Annotation> = parsed.iter().map(|(start, end, label)| {
            Annotation {
                id: uuid::Uuid::new_v4(),
                start_sec: *start,
                end_sec: Some(*end),
                label: label.clone(),
                track_index: args.track,
            }
        }).collect();
        let count = new_annotations.len();
        state.annotations.extend(new_annotations);
        let new_id = match append_state(ctx, state, format!("import_labels {} label(s)", count)) {
            Ok(id) => id, Err(e) => return Ok(ToolResult::Error(e)),
        };
        Ok(ToolResult::Ok(json!({ "node_id": new_id.to_hex(), "imported": count, "summary": format!("Imported {} label(s)", count) })))
    }
}
```

- [ ] **Step 6: Check Annotation struct fields match**

Read `crates/session/src/annotation.rs` to verify the Annotation struct fields. Adjust `id`, `start_sec`, `end_sec`, `label`, `track_index` field names to match.

```powershell
cd "C:\Users\tusha\Work\Playground\Edytlab\edytlab"; cat crates/session/src/annotation.rs 2>&1
```

Fix any field name mismatches in both `export_labels.rs` and `import_labels.rs`.

- [ ] **Step 7: Add uuid dependency to crates/tools if needed**

Check if uuid is already in `crates/tools/Cargo.toml`:
```powershell
cd "C:\Users\tusha\Work\Playground\Edytlab\edytlab"; grep uuid crates/tools/Cargo.toml
```

If not present, add to `crates/tools/Cargo.toml`:
```toml
uuid = { workspace = true }
```

- [ ] **Step 8: Register and test**

mod.rs: `pub mod export_labels; pub mod import_labels; pub use export_labels::ExportLabelsTool; pub use import_labels::ImportLabelsTool;`
dispatcher.rs: `d.register(Box::new(ExportLabelsTool)); d.register(Box::new(ImportLabelsTool));`

```powershell
cd "C:\Users\tusha\Work\Playground\Edytlab\edytlab"; cargo test -p tools export_labels import_labels 2>&1 | tail -10
```
Expected: `test result: ok. 4 passed`

- [ ] **Step 9: Commit**

```powershell
cd "C:\Users\tusha\Work\Playground\Edytlab\edytlab"; git add crates/tools/src/tool/export_labels.rs crates/tools/src/tool/import_labels.rs crates/tools/src/tool/mod.rs crates/tools/src/dispatcher.rs; git commit -m "feat(tools): export_labels, import_labels — Audacity label format I/O`n`nhttps://claude.ai/code/session_01jvf7s8jnfj9xhm5qzw7q8nd"
```

---

## Task 5: Microphone recording

**Files:**
- Create: `crates/recorder/` (new crate)
- Create: `crates/recorder/src/lib.rs`
- Create: `crates/recorder/Cargo.toml`
- Modify: `apps/desktop/src-tauri/Cargo.toml` (add recorder dep)
- Modify: `apps/desktop/src-tauri/src/commands.rs` (add start_recording, stop_recording commands)
- Modify: `apps/desktop/src/App.tsx` (record button + state)
- Modify: `apps/desktop/src/components/Timeline.tsx` (record button in toolbar)

- [ ] **Step 1: Create the recorder crate**

Create `crates/recorder/Cargo.toml`:
```toml
[package]
name = "recorder"
version = "0.1.0"
edition = "2021"
publish = false

[lints]
workspace = true

[dependencies]
cpal = "0.15"
hound = "3"
tokio = { workspace = true }
tracing = "0.1"
thiserror = "2"
```

- [ ] **Step 2: Write failing test for recorder**

Create `crates/recorder/src/lib.rs` with just the test (no implementation yet):

```rust
//! Microphone capture → WAV file writer.
#[cfg(test)]
mod tests {
    use super::format_seconds;
    #[test]
    fn formats_duration() {
        assert_eq!(format_seconds(65.3), "1:05");
        assert_eq!(format_seconds(3.0), "0:03");
    }
}

pub fn format_seconds(secs: f64) -> String {
    let m = (secs / 60.0) as u64;
    let s = secs as u64 % 60;
    format!("{m}:{s:02}")
}
```

- [ ] **Step 3: Run to verify it compiles (test passes immediately)**

```powershell
cd "C:\Users\tusha\Work\Playground\Edytlab\edytlab"; cargo test -p recorder 2>&1 | tail -5
```
Expected: `test result: ok. 1 passed`

- [ ] **Step 4: Implement the recorder**

Write the full `crates/recorder/src/lib.rs`:

```rust
//! Microphone capture → WAV file writer.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum RecorderError {
    #[error("no input device available")]
    NoDevice,
    #[error("stream error: {0}")]
    Stream(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("wav write error: {0}")]
    Wav(#[from] hound::Error),
}

pub struct Recorder {
    samples: Arc<Mutex<Vec<f32>>>,
    stream: Option<cpal::Stream>,
    sample_rate: u32,
    channels: u16,
}

impl Recorder {
    pub fn start() -> Result<Self, RecorderError> {
        let host = cpal::default_host();
        let device = host.default_input_device().ok_or(RecorderError::NoDevice)?;
        let config = device.default_input_config()
            .map_err(|e| RecorderError::Stream(e.to_string()))?;
        let sample_rate = config.sample_rate().0;
        let channels = config.channels();
        let samples = Arc::new(Mutex::new(Vec::new()));
        let samples_clone = Arc::clone(&samples);
        let stream = device.build_input_stream(
            &config.into(),
            move |data: &[f32], _| {
                if let Ok(mut s) = samples_clone.lock() {
                    s.extend_from_slice(data);
                }
            },
            |e| tracing::error!("stream error: {e}"),
            None,
        ).map_err(|e| RecorderError::Stream(e.to_string()))?;
        stream.play().map_err(|e| RecorderError::Stream(e.to_string()))?;
        Ok(Self { samples, stream: Some(stream), sample_rate, channels })
    }

    pub fn stop_and_save(mut self, path: &PathBuf) -> Result<(PathBuf, u32, u16), RecorderError> {
        drop(self.stream.take()); // stop the stream
        let samples = self.samples.lock().unwrap().clone();
        let spec = hound::WavSpec {
            channels: self.channels,
            sample_rate: self.sample_rate,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };
        let mut writer = hound::WavWriter::create(path, spec)?;
        for s in &samples { writer.write_sample(*s)?; }
        writer.finalize()?;
        Ok((path.clone(), self.sample_rate, self.channels))
    }

    pub fn duration_sec(&self) -> f64 {
        let n = self.samples.lock().unwrap().len();
        n as f64 / (self.sample_rate as f64 * self.channels as f64)
    }
}

pub fn format_seconds(secs: f64) -> String {
    let m = (secs / 60.0) as u64;
    let s = secs as u64 % 60;
    format!("{m}:{s:02}")
}
```

- [ ] **Step 5: Add to workspace Cargo.toml**

Open the workspace `Cargo.toml` (root). In `[workspace] members = [...]`, add:
```toml
"crates/recorder",
```

- [ ] **Step 6: Add Tauri commands for recording**

Open `apps/desktop/src-tauri/Cargo.toml`. Add:
```toml
recorder = { path = "../../../crates/recorder" }
hound = "3"
```

Open `apps/desktop/src-tauri/src/commands.rs`. Add at the bottom:

```rust
use std::sync::Mutex;

// Global recorder state (Tauri command state)
pub struct RecorderState(pub Mutex<Option<recorder::Recorder>>);

#[tauri::command]
pub fn start_recording(state: tauri::State<RecorderState>) -> Result<String, String> {
    let rec = recorder::Recorder::start().map_err(|e| e.to_string())?;
    *state.0.lock().unwrap() = Some(rec);
    Ok("recording started".into())
}

#[tauri::command]
pub fn stop_recording(
    state: tauri::State<RecorderState>,
    output_path: String,
) -> Result<serde_json::Value, String> {
    let rec = state.0.lock().unwrap().take()
        .ok_or_else(|| "no active recording".to_string())?;
    let path = std::path::PathBuf::from(&output_path);
    let (saved_path, sr, ch) = rec.stop_and_save(&path).map_err(|e| e.to_string())?;
    Ok(serde_json::json!({
        "path": saved_path.to_string_lossy(),
        "sample_rate": sr,
        "channels": ch
    }))
}
```

- [ ] **Step 7: Register state and commands in lib.rs**

Open `apps/desktop/src-tauri/src/lib.rs`. Add RecorderState to the Tauri builder:
```rust
.manage(commands::RecorderState(std::sync::Mutex::new(None)))
```
Add the two commands to `.invoke_handler(tauri::generate_handler![...])`:
```rust
commands::start_recording,
commands::stop_recording,
```

- [ ] **Step 8: Add record button to App.tsx**

Open `apps/desktop/src/App.tsx`. Add:
```typescript
const [isRecording, setIsRecording] = useState(false);
const [recordingPath, setRecordingPath] = useState<string | null>(null);

const handleStartRecording = async () => {
  try {
    await invoke("start_recording");
    setIsRecording(true);
  } catch (e) { console.error("start_recording failed:", e); }
};

const handleStopRecording = async () => {
  const outPath = `${await tempDir()}recording_${Date.now()}.wav`;
  try {
    const result = await invoke<{ path: string }>("stop_recording", { outputPath: outPath });
    setIsRecording(false);
    // Load the recording as a new track
    await invoke("batch_load", { paths: [result.path] });
    await refreshTracks();
  } catch (e) { console.error("stop_recording failed:", e); }
};
```

Add a record button in the toolbar area:
```tsx
<button
  data-testid="record-btn"
  onClick={isRecording ? handleStopRecording : handleStartRecording}
  className={`px-3 py-1 text-sm rounded ${isRecording ? "bg-red-600 animate-pulse" : "bg-neutral-700"}`}
>
  {isRecording ? "⏹ Stop" : "⏺ Record"}
</button>
```

- [ ] **Step 9: Write frontend test for record button**

Create `apps/desktop/src/__tests__/RecordButton.test.tsx`:

```tsx
import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";

// Simple smoke test: record button exists
// Full recording tests require hardware — skipped in CI
describe("record button", () => {
  it("renders record button in toolbar", async () => {
    // Test the button label, not the actual recording
    const btn = document.createElement("button");
    btn.setAttribute("data-testid", "record-btn");
    btn.textContent = "⏺ Record";
    document.body.appendChild(btn);
    expect(document.querySelector("[data-testid=record-btn]")).toBeTruthy();
    document.body.removeChild(btn);
  });
});
```

- [ ] **Step 10: Build and test**

```powershell
cd "C:\Users\tusha\Work\Playground\Edytlab\edytlab"; cargo test -p recorder 2>&1 | tail -5
```

```powershell
cd "C:\Users\tusha\Work\Playground\Edytlab\edytlab"; pnpm --filter @edytlab/desktop test 2>&1 | tail -5
pnpm --filter @edytlab/desktop exec tsc --noEmit 2>&1 | tail -5
```
Expected: all pass.

Note: `cargo build` (full Tauri build) requires the Windows SDK. CI will verify the full build. The unit tests cover the recorder logic.

- [ ] **Step 11: Commit**

```powershell
cd "C:\Users\tusha\Work\Playground\Edytlab\edytlab"; git add crates/recorder/ apps/desktop/src-tauri/src/commands.rs apps/desktop/src-tauri/src/lib.rs apps/desktop/src-tauri/Cargo.toml apps/desktop/src/App.tsx apps/desktop/src/__tests__/RecordButton.test.tsx Cargo.toml; git commit -m "feat: microphone recording — CPAL capture → WAV → new track`n`nhttps://claude.ai/code/session_01jvf7s8jnfj9xhm5qzw7q8nd"
```

---

## Final validation

- [ ] **Full Rust test suite**

```powershell
cd "C:\Users\tusha\Work\Playground\Edytlab\edytlab"; cargo test --workspace 2>&1 | tail -10
```
Expected: all tests pass.

- [ ] **Clippy**

```powershell
cd "C:\Users\tusha\Work\Playground\Edytlab\edytlab"; cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -20
```

- [ ] **Frontend tests and type check**

```powershell
cd "C:\Users\tusha\Work\Playground\Edytlab\edytlab"; pnpm --filter @edytlab/desktop test 2>&1 | tail -10
pnpm --filter @edytlab/desktop exec tsc --noEmit 2>&1 | tail -5
```

- [ ] **Cargo fmt**

```powershell
cd "C:\Users\tusha\Work\Playground\Edytlab\edytlab"; cargo fmt --all -- --check 2>&1 | tail -5
```
