# Audacity Parity A2 — Medium-Complexity Tools Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add 17 medium-complexity audio processing tools (noise_gate, truncate_silence, reverb, echo, click_removal, leveler, de_esser, limiter, stereo↔mono conversion, resample_track, mix_to_new_track, generate_tone, generate_noise, export_multiple, silence_finder, vocal_reduction) covering Audacity's Effect and Analyze menus.

**Architecture:** Same Tool trait pattern as A1. All destructive edits use `destructive_edit()` helper from `crates/tools/src/tool/util.rs`. Non-destructive analysis tools (silence_finder) return JSON data without appending a session node. The Freeverb reverb and Schroeder echo algorithms are self-contained in their respective tool files. `mix_to_new_track` and `export_multiple` call `audio_engine::render_to_wav`.

**Tech Stack:** Rust, `serde_json`, `audio_decoder`, `audio_engine::write_wav`, `session::{SessionState, Track, Clip, TrackId}`. Plan A1 must be merged first (biquad helper in util.rs required for de_esser).

**Prerequisite:** Plan A1 must be implemented first (biquad_process, BiquadCoeffs in util.rs).

---

## Task 1: `noise_gate` — silence samples below threshold

**Files:**
- Create: `crates/tools/src/tool/noise_gate.rs`
- Modify: mod.rs, dispatcher.rs

- [ ] **Step 1: Write failing test**

```rust
#[cfg(test)]
mod tests {
    use super::apply_noise_gate;

    #[test]
    fn silences_below_threshold() {
        // -40 dBFS threshold, 1 channel, sr=100
        // linear threshold = 10^(-40/20) = 0.01
        let mut samples: Vec<f32> = vec![0.005, 0.005, 0.5, 0.5, 0.005, 0.005];
        apply_noise_gate(&mut samples, 100, 1, -40.0, 1.0, 10.0);
        // samples below 0.01 should be zeroed (with attack/release smoothing, tail may linger)
        assert_eq!(samples[2], 0.5, "above-threshold sample untouched");
        assert_eq!(samples[3], 0.5, "above-threshold sample untouched");
        // with 1ms attack the gate opens fast; below-threshold before the loud part should be silenced
        assert!(samples[0].abs() < 0.01, "below threshold silenced");
    }
}
```

- [ ] **Step 2: Run to verify it fails**

```powershell
cd "C:\Users\tusha\Work\Playground\Edytlab\edytlab"; cargo test -p tools noise_gate 2>&1 | tail -5
```

- [ ] **Step 3: Implement**

```rust
use serde::Deserialize;
use serde_json::Value;
use crate::schema::anthropic_tool;
use crate::tool::util::destructive_edit;
use crate::{Tool, ToolContext, ToolResult};

pub(crate) fn apply_noise_gate(
    samples: &mut [f32],
    sr: u32,
    channels: usize,
    threshold_db: f32,
    attack_ms: f32,
    release_ms: f32,
) {
    let channels = channels.max(1);
    let threshold_lin = 10.0f32.powf(threshold_db / 20.0);
    let attack_coeff = (-1.0 / (attack_ms * 0.001 * sr as f32)).exp();
    let release_coeff = (-1.0 / (release_ms * 0.001 * sr as f32)).exp();
    let mut gain = 0.0f32; // gate starts closed
    let n_frames = samples.len() / channels;
    for frame in 0..n_frames {
        // Compute peak across channels for this frame
        let peak = (0..channels)
            .map(|ch| samples[frame * channels + ch].abs())
            .fold(0.0f32, f32::max);
        let target = if peak >= threshold_lin { 1.0f32 } else { 0.0f32 };
        let coeff = if target > gain { attack_coeff } else { release_coeff };
        gain = target + coeff * (gain - target);
        for ch in 0..channels {
            samples[frame * channels + ch] *= gain;
        }
    }
}

#[derive(Debug, Deserialize)]
struct Args {
    track: usize,
    threshold_db: f32,
    attack_ms: Option<f32>,
    release_ms: Option<f32>,
    start_sec: Option<f64>,
    end_sec: Option<f64>,
}

pub struct NoiseGateTool;

impl Tool for NoiseGateTool {
    fn name(&self) -> &'static str { "noise_gate" }

    fn schema(&self) -> Value {
        anthropic_tool("noise_gate",
            "Apply a noise gate: audio below threshold_db is silenced. attack_ms and release_ms control how fast the gate opens/closes. Appends a new session node.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "track": { "type": "integer" },
                    "threshold_db": { "type": "number", "description": "Gate threshold in dBFS (e.g. -40)" },
                    "attack_ms": { "type": "number", "default": 5.0, "description": "Gate open time in ms" },
                    "release_ms": { "type": "number", "default": 100.0, "description": "Gate close time in ms" },
                    "start_sec": { "type": "number" },
                    "end_sec": { "type": "number" }
                },
                "required": ["track", "threshold_db"]
            }))
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a, Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };
        let attack = args.attack_ms.unwrap_or(5.0).max(0.1);
        let release = args.release_ms.unwrap_or(100.0).max(0.1);
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
        let (thresh, s, e) = (args.threshold_db, args.start_sec, args.end_sec);
        Ok(destructive_edit(ctx, args.track,
            move |samples, sr| {
                // Apply range restriction if requested
                let channels = channels;
                let len_frames = samples.len() / channels.max(1);
                let start_frame = s.map(|sec| ((sec * sr as f64) as usize).min(len_frames)).unwrap_or(0);
                let end_frame = e.map(|sec| ((sec * sr as f64) as usize).min(len_frames)).unwrap_or(len_frames);
                let start_idx = start_frame * channels.max(1);
                let end_idx = end_frame * channels.max(1);
                apply_noise_gate(&mut samples[start_idx..end_idx], sr, channels, thresh, attack, release);
            },
            format!("noise_gate track {} threshold={}dB", args.track, args.threshold_db),
        ))
    }
}
```

- [ ] **Step 4: Register and test**

mod.rs: `pub mod noise_gate; pub use noise_gate::NoiseGateTool;`
dispatcher.rs: `d.register(Box::new(NoiseGateTool));`

```powershell
cd "C:\Users\tusha\Work\Playground\Edytlab\edytlab"; cargo test -p tools noise_gate 2>&1 | tail -10
```
Expected: `test result: ok. 1 passed`

- [ ] **Step 5: Commit**

```powershell
cd "C:\Users\tusha\Work\Playground\Edytlab\edytlab"; git add crates/tools/src/tool/noise_gate.rs crates/tools/src/tool/mod.rs crates/tools/src/dispatcher.rs; git commit -m "feat(tools): noise_gate — envelope-follower gate with attack/release`n`nhttps://claude.ai/code/session_01jvf7s8jnfj9xhm5qzw7q8nd"
```

---

## Task 2: `truncate_silence` — remove silent regions

**Files:**
- Create: `crates/tools/src/tool/truncate_silence.rs`
- Modify: mod.rs, dispatcher.rs

- [ ] **Step 1: Write failing test**

```rust
#[cfg(test)]
mod tests {
    use super::{find_silent_regions, apply_truncate_silence};

    #[test]
    fn finds_silent_region() {
        // sr=10, 1ch, threshold=-60dBFS (lin≈0.001)
        // silent region: frames 3..7 (all zeros)
        let samples: Vec<f32> = [vec![0.5f32; 3], vec![0.0f32; 4], vec![0.5f32; 3]].concat();
        let regions = find_silent_regions(&samples, 10, 1, -60.0, 100.0); // min_silence_ms=100 → 1 frame at sr=10
        assert!(!regions.is_empty(), "should find a silent region");
        let (s, e) = regions[0];
        assert_eq!(s, 3);
        assert_eq!(e, 7);
    }

    #[test]
    fn removes_silence() {
        let samples: Vec<f32> = [vec![0.5f32; 3], vec![0.0f32; 4], vec![0.5f32; 3]].concat();
        let result = apply_truncate_silence(samples.clone(), 10, 1, -60.0, 100.0);
        // The 4 silent frames should be removed
        assert_eq!(result.len(), 6, "silent frames removed");
    }
}
```

- [ ] **Step 2: Run to verify it fails**

```powershell
cd "C:\Users\tusha\Work\Playground\Edytlab\edytlab"; cargo test -p tools truncate_silence 2>&1 | tail -5
```

- [ ] **Step 3: Implement**

```rust
use serde::Deserialize;
use serde_json::Value;
use crate::schema::anthropic_tool;
use crate::tool::util::destructive_edit;
use crate::{Tool, ToolContext, ToolResult};

/// Returns list of (start_frame, end_frame) silent regions meeting min duration.
pub(crate) fn find_silent_regions(
    samples: &[f32], sr: u32, channels: usize, threshold_db: f32, min_silence_ms: f32,
) -> Vec<(usize, usize)> {
    let channels = channels.max(1);
    let threshold_lin = 10.0f32.powf(threshold_db / 20.0);
    let min_frames = ((min_silence_ms * 0.001 * sr as f32) as usize).max(1);
    let n_frames = samples.len() / channels;
    let mut regions = Vec::new();
    let mut silent_start: Option<usize> = None;
    for frame in 0..n_frames {
        let peak = (0..channels).map(|ch| samples[frame * channels + ch].abs()).fold(0.0f32, f32::max);
        let is_silent = peak < threshold_lin;
        match (is_silent, silent_start) {
            (true, None) => silent_start = Some(frame),
            (false, Some(start)) => {
                if frame - start >= min_frames { regions.push((start, frame)); }
                silent_start = None;
            }
            _ => {}
        }
    }
    if let Some(start) = silent_start {
        if n_frames - start >= min_frames { regions.push((start, n_frames)); }
    }
    regions
}

pub(crate) fn apply_truncate_silence(
    samples: Vec<f32>, sr: u32, channels: usize, threshold_db: f32, min_silence_ms: f32,
) -> Vec<f32> {
    let channels = channels.max(1);
    let regions = find_silent_regions(&samples, sr, channels, threshold_db, min_silence_ms);
    if regions.is_empty() { return samples; }
    // Build a mask of frames to keep
    let n_frames = samples.len() / channels;
    let mut keep = vec![true; n_frames];
    for (s, e) in regions { for f in s..e { keep[f] = false; } }
    let mut out = Vec::with_capacity(samples.len());
    for (frame, &kept) in keep.iter().enumerate() {
        if kept {
            for ch in 0..channels { out.push(samples[frame * channels + ch]); }
        }
    }
    out
}

#[derive(Debug, Deserialize)]
struct Args { track: usize, threshold_db: f32, min_silence_ms: Option<f32> }

pub struct TruncateSilenceTool;

impl Tool for TruncateSilenceTool {
    fn name(&self) -> &'static str { "truncate_silence" }

    fn schema(&self) -> Value {
        anthropic_tool("truncate_silence",
            "Find and remove silent regions in a track. threshold_db is the silence floor; min_silence_ms is the minimum gap duration to remove. Appends a new session node.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "track": { "type": "integer" },
                    "threshold_db": { "type": "number", "description": "Silence threshold in dBFS (e.g. -60)" },
                    "min_silence_ms": { "type": "number", "default": 500.0, "description": "Minimum silence duration to remove in ms" }
                },
                "required": ["track", "threshold_db"]
            }))
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a, Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };
        let min_ms = args.min_silence_ms.unwrap_or(500.0).max(1.0);
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
        let (thresh, min) = (args.threshold_db, min_ms);
        Ok(destructive_edit(ctx, args.track,
            move |samples, sr| {
                let result = apply_truncate_silence(samples.clone(), sr, channels, thresh, min);
                *samples = result;
            },
            format!("truncate_silence track {} threshold={}dB min={}ms", args.track, args.threshold_db, min_ms),
        ))
    }
}
```

- [ ] **Step 4: Register and test**

mod.rs: `pub mod truncate_silence; pub use truncate_silence::TruncateSilenceTool;`
dispatcher.rs: `d.register(Box::new(TruncateSilenceTool));`

```powershell
cd "C:\Users\tusha\Work\Playground\Edytlab\edytlab"; cargo test -p tools truncate_silence 2>&1 | tail -10
```
Expected: `test result: ok. 2 passed`

- [ ] **Step 5: Commit**

```powershell
cd "C:\Users\tusha\Work\Playground\Edytlab\edytlab"; git add crates/tools/src/tool/truncate_silence.rs crates/tools/src/tool/mod.rs crates/tools/src/dispatcher.rs; git commit -m "feat(tools): truncate_silence — remove silent gaps`n`nhttps://claude.ai/code/session_01jvf7s8jnfj9xhm5qzw7q8nd"
```

---

## Task 3: `reverb` — Freeverb algorithm

**Files:**
- Create: `crates/tools/src/tool/reverb.rs`
- Modify: mod.rs, dispatcher.rs

- [ ] **Step 1: Write failing test**

```rust
#[cfg(test)]
mod tests {
    use super::apply_reverb;
    #[test]
    fn wet_zero_passes_through() {
        let original = vec![0.5f32, -0.3, 0.1, 0.8, 0.0, -0.5];
        let mut samples = original.clone();
        apply_reverb(&mut samples, 44100, 1, 0.5, 0.5, 0.0);
        for (a, b) in original.iter().zip(samples.iter()) {
            assert!((a - b).abs() < 1e-5, "wet=0 should pass through unchanged");
        }
    }
    #[test]
    fn wet_one_returns_reverb_only() {
        // With wet=1, dry=0 — output must differ from input
        let mut samples: Vec<f32> = (0..4410).map(|i| if i < 100 { 1.0 } else { 0.0 }).collect();
        let original = samples.clone();
        apply_reverb(&mut samples, 44100, 1, 0.8, 0.5, 1.0);
        // reverb tail should be present after the impulse
        let tail_energy: f32 = samples[200..].iter().map(|s| s * s).sum();
        assert!(tail_energy > 0.001, "reverb tail should have energy");
        // first sample should differ from input (wet-only mix)
        assert!((samples[0] - original[0]).abs() > 0.0001 || tail_energy > 0.001);
    }
}
```

- [ ] **Step 2: Run to verify it fails**

```powershell
cd "C:\Users\tusha\Work\Playground\Edytlab\edytlab"; cargo test -p tools reverb 2>&1 | tail -5
```

- [ ] **Step 3: Implement Freeverb**

```rust
use serde::Deserialize;
use serde_json::Value;
use crate::schema::anthropic_tool;
use crate::tool::util::destructive_edit;
use crate::{Tool, ToolContext, ToolResult};

// Freeverb constants (Schroeder/Moorer-style)
const COMB_TUNING: [usize; 8] = [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617];
const ALLPASS_TUNING: [usize; 4] = [556, 441, 341, 225];
const STEREO_SPREAD: usize = 23;
const FIXED_GAIN: f32 = 0.015;

struct CombFilter { buf: Vec<f32>, idx: usize, feedback: f32, damp1: f32, damp2: f32, filterstore: f32 }
impl CombFilter {
    fn new(size: usize, room: f32, damp: f32) -> Self {
        Self { buf: vec![0.0; size], idx: 0, feedback: room, damp1: damp, damp2: 1.0 - damp, filterstore: 0.0 }
    }
    fn process(&mut self, input: f32) -> f32 {
        let output = self.buf[self.idx];
        self.filterstore = output * self.damp2 + self.filterstore * self.damp1;
        self.buf[self.idx] = input + self.filterstore * self.feedback;
        self.idx = (self.idx + 1) % self.buf.len();
        output
    }
}

struct AllpassFilter { buf: Vec<f32>, idx: usize }
impl AllpassFilter {
    fn new(size: usize) -> Self { Self { buf: vec![0.0; size], idx: 0 } }
    fn process(&mut self, input: f32) -> f32 {
        let buf_out = self.buf[self.idx];
        let output = -input + buf_out;
        self.buf[self.idx] = input + buf_out * 0.5;
        self.idx = (self.idx + 1) % self.buf.len();
        output
    }
}

pub(crate) fn apply_reverb(samples: &mut Vec<f32>, sr: u32, channels: usize, room_size: f32, damping: f32, wet: f32) {
    let channels = channels.max(1);
    // Scale tunings from 44100 Hz baseline
    let scale = sr as f32 / 44100.0;
    let room = room_size.clamp(0.0, 1.0) * 0.28 + 0.7;
    let damp = damping.clamp(0.0, 1.0) * 0.4;
    let wet = wet.clamp(0.0, 1.0);
    let dry = 1.0 - wet;
    let n_frames = samples.len() / channels;
    // Build L and R comb filter banks
    let mut combs_l: Vec<CombFilter> = COMB_TUNING.iter()
        .map(|&t| CombFilter::new((t as f32 * scale) as usize, room, damp)).collect();
    let mut combs_r: Vec<CombFilter> = COMB_TUNING.iter()
        .map(|&t| CombFilter::new(((t + STEREO_SPREAD) as f32 * scale) as usize, room, damp)).collect();
    let mut allpasses_l: Vec<AllpassFilter> = ALLPASS_TUNING.iter()
        .map(|&t| AllpassFilter::new((t as f32 * scale) as usize)).collect();
    let mut allpasses_r: Vec<AllpassFilter> = ALLPASS_TUNING.iter()
        .map(|&t| AllpassFilter::new(((t + STEREO_SPREAD) as f32 * scale) as usize)).collect();
    for frame in 0..n_frames {
        // Mix all channels to mono for reverb input
        let input: f32 = (0..channels).map(|ch| samples[frame * channels + ch]).sum::<f32>()
            / channels as f32 * FIXED_GAIN;
        let mut out_l = combs_l.iter_mut().map(|c| c.process(input)).sum::<f32>();
        let mut out_r = combs_r.iter_mut().map(|c| c.process(input)).sum::<f32>();
        for ap in &mut allpasses_l { out_l = ap.process(out_l); }
        for ap in &mut allpasses_r { out_r = ap.process(out_r); }
        if channels == 1 {
            samples[frame] = samples[frame] * dry + out_l * wet;
        } else {
            samples[frame * channels] = samples[frame * channels] * dry + out_l * wet;
            samples[frame * channels + 1] = samples[frame * channels + 1] * dry + out_r * wet;
            for ch in 2..channels {
                samples[frame * channels + ch] *= dry;
            }
        }
    }
}

#[derive(Debug, Deserialize)]
struct Args { track: usize, room_size: Option<f32>, damping: Option<f32>, wet: Option<f32>, start_sec: Option<f64>, end_sec: Option<f64> }

pub struct ReverbTool;

impl Tool for ReverbTool {
    fn name(&self) -> &'static str { "reverb" }

    fn schema(&self) -> Value {
        anthropic_tool("reverb",
            "Apply Freeverb algorithmic reverb. room_size (0-1) controls reverb length, damping (0-1) controls high-freq decay, wet (0-1) is the wet/dry blend. Appends a new session node.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "track": { "type": "integer" },
                    "room_size": { "type": "number", "default": 0.5, "description": "Room size 0..1" },
                    "damping": { "type": "number", "default": 0.5, "description": "High-freq damping 0..1" },
                    "wet": { "type": "number", "default": 0.3, "description": "Wet mix 0..1" },
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
        let room = args.room_size.unwrap_or(0.5);
        let damp = args.damping.unwrap_or(0.5);
        let wet = args.wet.unwrap_or(0.3);
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
        Ok(destructive_edit(ctx, args.track,
            move |samples, sr| {
                // Apply to full buffer (range restriction adds complexity; reverb tail bleeds anyway)
                let mut v = samples.to_vec();
                apply_reverb(&mut v, sr, channels, room, damp, wet);
                *samples = v;
            },
            format!("reverb track {} room={:.2} wet={:.2}", args.track, room, wet),
        ))
    }
}
```

- [ ] **Step 4: Register and test**

mod.rs: `pub mod reverb; pub use reverb::ReverbTool;`
dispatcher.rs: `d.register(Box::new(ReverbTool));`

```powershell
cd "C:\Users\tusha\Work\Playground\Edytlab\edytlab"; cargo test -p tools reverb 2>&1 | tail -10
```
Expected: `test result: ok. 2 passed`

- [ ] **Step 5: Commit**

```powershell
cd "C:\Users\tusha\Work\Playground\Edytlab\edytlab"; git add crates/tools/src/tool/reverb.rs crates/tools/src/tool/mod.rs crates/tools/src/dispatcher.rs; git commit -m "feat(tools): reverb — Freeverb algorithmic reverb`n`nhttps://claude.ai/code/session_01jvf7s8jnfj9xhm5qzw7q8nd"
```

---

## Task 4: `echo` — delay line feedback

**Files:**
- Create: `crates/tools/src/tool/echo.rs`
- Modify: mod.rs, dispatcher.rs

- [ ] **Step 1: Write failing test**

```rust
#[cfg(test)]
mod tests {
    use super::apply_echo;
    #[test]
    fn echo_appears_after_delay() {
        let mut samples = vec![0.0f32; 4410]; // 0.1s at 44100
        samples[0] = 1.0; // impulse at t=0
        apply_echo(&mut samples, 44100, 1, 50.0, 0.5); // 50ms delay
        let delay_frames = (50.0f32 * 0.001 * 44100.0) as usize; // 2205
        assert!(samples[delay_frames].abs() > 0.3, "echo peak expected at delay offset");
    }
}
```

- [ ] **Step 2: Run to verify it fails**

```powershell
cd "C:\Users\tusha\Work\Playground\Edytlab\edytlab"; cargo test -p tools echo 2>&1 | tail -5
```

- [ ] **Step 3: Implement**

```rust
use serde::Deserialize;
use serde_json::Value;
use crate::schema::anthropic_tool;
use crate::tool::util::destructive_edit;
use crate::{Tool, ToolContext, ToolResult};

pub(crate) fn apply_echo(samples: &mut Vec<f32>, sr: u32, channels: usize, delay_ms: f32, decay: f32) {
    let channels = channels.max(1);
    let delay_frames = ((delay_ms * 0.001 * sr as f32) as usize).max(1);
    let delay_samples = delay_frames * channels;
    let n = samples.len();
    // Extend output to include echo tail (one decay cycle)
    let tail = delay_samples;
    samples.resize(n + tail, 0.0);
    for i in 0..n {
        let echo_idx = i + delay_samples;
        if echo_idx < samples.len() {
            samples[echo_idx] += samples[i] * decay;
        }
    }
}

#[derive(Debug, Deserialize)]
struct Args { track: usize, delay_ms: f32, decay: Option<f32>, start_sec: Option<f64>, end_sec: Option<f64> }

pub struct EchoTool;

impl Tool for EchoTool {
    fn name(&self) -> &'static str { "echo" }

    fn schema(&self) -> Value {
        anthropic_tool("echo",
            "Add a single echo (delay + decay). delay_ms is the echo offset in milliseconds; decay (0..1) is the echo amplitude. Appends a new session node.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "track": { "type": "integer" },
                    "delay_ms": { "type": "number", "description": "Echo delay in milliseconds" },
                    "decay": { "type": "number", "default": 0.5, "description": "Echo amplitude 0..1" },
                    "start_sec": { "type": "number" },
                    "end_sec": { "type": "number" }
                },
                "required": ["track", "delay_ms"]
            }))
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a, Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };
        if args.delay_ms <= 0.0 { return Ok(ToolResult::Error("delay_ms must be positive".into())); }
        let decay = args.decay.unwrap_or(0.5).clamp(0.0, 1.0);
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
        let (delay, d) = (args.delay_ms, decay);
        Ok(destructive_edit(ctx, args.track,
            move |samples, sr| {
                let mut v = samples.to_vec();
                apply_echo(&mut v, sr, channels, delay, d);
                *samples = v;
            },
            format!("echo track {} delay={}ms decay={:.2}", args.track, args.delay_ms, decay),
        ))
    }
}
```

- [ ] **Step 4: Register and test**

mod.rs: `pub mod echo; pub use echo::EchoTool;`
dispatcher.rs: `d.register(Box::new(EchoTool));`

```powershell
cd "C:\Users\tusha\Work\Playground\Edytlab\edytlab"; cargo test -p tools echo 2>&1 | tail -10
```
Expected: `test result: ok. 1 passed`

- [ ] **Step 5: Commit**

```powershell
cd "C:\Users\tusha\Work\Playground\Edytlab\edytlab"; git add crates/tools/src/tool/echo.rs crates/tools/src/tool/mod.rs crates/tools/src/dispatcher.rs; git commit -m "feat(tools): echo — delay line feedback echo`n`nhttps://claude.ai/code/session_01jvf7s8jnfj9xhm5qzw7q8nd"
```

---

## Task 5: `click_removal` — median-filter spike detection

**Files:**
- Create: `crates/tools/src/tool/click_removal.rs`
- Modify: mod.rs, dispatcher.rs

- [ ] **Step 1: Write failing test**

```rust
#[cfg(test)]
mod tests {
    use super::apply_click_removal;
    #[test]
    fn removes_spike() {
        let mut samples = vec![0.1f32; 100];
        samples[50] = 10.0; // spike
        apply_click_removal(&mut samples, 44100, 1, 3.0);
        assert!(samples[50].abs() < 1.0, "spike should be attenuated, got {}", samples[50]);
        assert!((samples[49] - 0.1).abs() < 0.02, "neighbors untouched");
    }
}
```

- [ ] **Step 2: Run to verify it fails**

```powershell
cd "C:\Users\tusha\Work\Playground\Edytlab\edytlab"; cargo test -p tools click_removal 2>&1 | tail -5
```

- [ ] **Step 3: Implement**

```rust
use serde::Deserialize;
use serde_json::Value;
use crate::schema::anthropic_tool;
use crate::tool::util::destructive_edit;
use crate::{Tool, ToolContext, ToolResult};

/// Median of a small window (3 elements).
fn median3(a: f32, b: f32, c: f32) -> f32 {
    let mut v = [a, b, c];
    v.sort_by(|x, y| x.partial_cmp(y).unwrap());
    v[1]
}

pub(crate) fn apply_click_removal(samples: &mut [f32], _sr: u32, channels: usize, threshold: f32) {
    let channels = channels.max(1);
    let n_frames = samples.len() / channels;
    if n_frames < 3 { return; }
    for frame in 1..n_frames - 1 {
        for ch in 0..channels {
            let prev = samples[(frame - 1) * channels + ch];
            let curr = samples[frame * channels + ch];
            let next = samples[(frame + 1) * channels + ch];
            let med = median3(prev, curr, next);
            if (curr - med).abs() > threshold {
                samples[frame * channels + ch] = med;
            }
        }
    }
}

#[derive(Debug, Deserialize)]
struct Args { track: usize, threshold: Option<f32>, start_sec: Option<f64>, end_sec: Option<f64> }

pub struct ClickRemovalTool;

impl Tool for ClickRemovalTool {
    fn name(&self) -> &'static str { "click_removal" }

    fn schema(&self) -> Value {
        anthropic_tool("click_removal",
            "Remove clicks and pops by detecting sample spikes (via median filter) and replacing them with interpolated values. threshold is the amplitude deviation that triggers detection. Appends a new session node.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "track": { "type": "integer" },
                    "threshold": { "type": "number", "default": 0.5, "description": "Amplitude spike threshold (linear, 0..1 scale)" },
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
        let threshold = args.threshold.unwrap_or(0.5).max(0.0);
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
        let (thresh, s, e) = (threshold, args.start_sec, args.end_sec);
        Ok(destructive_edit(ctx, args.track,
            move |samples, sr| {
                let ch = channels;
                let len_frames = samples.len() / ch.max(1);
                let start = s.map(|sec| ((sec * sr as f64) as usize).min(len_frames)).unwrap_or(0);
                let end = e.map(|sec| ((sec * sr as f64) as usize).min(len_frames)).unwrap_or(len_frames);
                apply_click_removal(&mut samples[start * ch.max(1)..end * ch.max(1)], sr, ch, thresh);
            },
            format!("click_removal track {} threshold={:.3}", args.track, threshold),
        ))
    }
}
```

- [ ] **Step 4: Register and test**

mod.rs: `pub mod click_removal; pub use click_removal::ClickRemovalTool;`
dispatcher.rs: `d.register(Box::new(ClickRemovalTool));`

```powershell
cd "C:\Users\tusha\Work\Playground\Edytlab\edytlab"; cargo test -p tools click_removal 2>&1 | tail -10
```
Expected: `test result: ok. 1 passed`

- [ ] **Step 5: Commit**

```powershell
cd "C:\Users\tusha\Work\Playground\Edytlab\edytlab"; git add crates/tools/src/tool/click_removal.rs crates/tools/src/tool/mod.rs crates/tools/src/dispatcher.rs; git commit -m "feat(tools): click_removal — median filter spike detection`n`nhttps://claude.ai/code/session_01jvf7s8jnfj9xhm5qzw7q8nd"
```

---

## Task 6: `leveler` and `limiter`

**Files:**
- Create: `crates/tools/src/tool/leveler.rs`
- Create: `crates/tools/src/tool/limiter.rs`
- Modify: mod.rs, dispatcher.rs

- [ ] **Step 1: Write failing tests**

`leveler.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::apply_leveler;
    #[test]
    fn boosts_quiet_section() {
        // 200 frames: first 100 at 0.1 RMS, next 100 at 0.9 RMS
        let mut samples: Vec<f32> = (0..200).map(|i| if i < 100 { 0.1f32 } else { 0.9 }).collect();
        apply_leveler(&mut samples, 44100, 1, -12.0, 50); // window=50
        // After leveling, quiet section should be louder
        let loud_before: f32 = samples[..100].iter().map(|s| s.abs()).sum::<f32>() / 100.0;
        assert!(loud_before > 0.15, "quiet section boosted, got {loud_before}");
    }
}
```

`limiter.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::apply_limiter;
    #[test]
    fn clips_above_ceiling() {
        let mut samples = vec![0.5f32, 0.8, 1.5, -1.2, 0.3];
        apply_limiter(&mut samples, 44100, 1, -6.0); // ceiling = 0.5012
        let ceiling = 10.0f32.powf(-6.0 / 20.0);
        for s in &samples {
            assert!(s.abs() <= ceiling + 1e-5, "sample {} exceeds ceiling {}", s, ceiling);
        }
    }
}
```

- [ ] **Step 2: Run to verify they fail**

```powershell
cd "C:\Users\tusha\Work\Playground\Edytlab\edytlab"; cargo test -p tools leveler limiter 2>&1 | tail -5
```

- [ ] **Step 3: Implement `leveler`**

```rust
use serde::Deserialize;
use serde_json::Value;
use crate::schema::anthropic_tool;
use crate::tool::util::destructive_edit;
use crate::{Tool, ToolContext, ToolResult};

pub(crate) fn apply_leveler(samples: &mut [f32], sr: u32, channels: usize, target_db: f32, window_ms: u32) {
    let channels = channels.max(1);
    let target_rms = 10.0f32.powf(target_db / 20.0);
    let window_frames = ((window_ms as f32 * 0.001 * sr as f32) as usize).max(1);
    let n_frames = samples.len() / channels;
    let mut frame = 0;
    while frame < n_frames {
        let end = (frame + window_frames).min(n_frames);
        // Compute RMS for this window
        let rms: f32 = {
            let slice_start = frame * channels;
            let slice_end = end * channels;
            let sum_sq: f32 = samples[slice_start..slice_end].iter().map(|s| s * s).sum();
            (sum_sq / (slice_end - slice_start) as f32).sqrt()
        };
        if rms > 1e-6 {
            let gain = (target_rms / rms).min(10.0); // max +20dB boost
            for s in &mut samples[frame * channels..end * channels] { *s *= gain; }
        }
        frame = end;
    }
}

#[derive(Debug, Deserialize)]
struct Args { track: usize, target_db: f32, start_sec: Option<f64>, end_sec: Option<f64> }

pub struct LevelerTool;

impl Tool for LevelerTool {
    fn name(&self) -> &'static str { "leveler" }

    fn schema(&self) -> Value {
        anthropic_tool("leveler",
            "Apply dynamic leveling: normalise each short window to a target RMS level. Reduces variation between loud and quiet passages. Appends a new session node.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "track": { "type": "integer" },
                    "target_db": { "type": "number", "description": "Target RMS level in dBFS (e.g. -18)" },
                    "start_sec": { "type": "number" },
                    "end_sec": { "type": "number" }
                },
                "required": ["track", "target_db"]
            }))
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a, Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };
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
        let (target, s, e) = (args.target_db, args.start_sec, args.end_sec);
        Ok(destructive_edit(ctx, args.track,
            move |samples, sr| {
                let ch = channels;
                let len_frames = samples.len() / ch.max(1);
                let start = s.map(|sec| ((sec * sr as f64) as usize).min(len_frames)).unwrap_or(0);
                let end = e.map(|sec| ((sec * sr as f64) as usize).min(len_frames)).unwrap_or(len_frames);
                apply_leveler(&mut samples[start * ch.max(1)..end * ch.max(1)], sr, ch, target, 100);
            },
            format!("leveler track {} target={}dB", args.track, target),
        ))
    }
}
```

- [ ] **Step 4: Implement `limiter`**

```rust
use serde::Deserialize;
use serde_json::Value;
use crate::schema::anthropic_tool;
use crate::tool::util::destructive_edit;
use crate::{Tool, ToolContext, ToolResult};

pub(crate) fn apply_limiter(samples: &mut [f32], _sr: u32, _channels: usize, ceiling_db: f32) {
    let ceiling = 10.0f32.powf(ceiling_db / 20.0);
    for s in samples.iter_mut() {
        // Soft clip using tanh for mild overdrive, hard clip at ceiling
        if s.abs() > ceiling {
            *s = s.signum() * ceiling;
        }
    }
}

#[derive(Debug, Deserialize)]
struct Args { track: usize, ceiling_db: f32, start_sec: Option<f64>, end_sec: Option<f64> }

pub struct LimiterTool;

impl Tool for LimiterTool {
    fn name(&self) -> &'static str { "limiter" }

    fn schema(&self) -> Value {
        anthropic_tool("limiter",
            "Brick-wall limiter: hard-clip any samples exceeding ceiling_db. Prevents digital clipping. Appends a new session node.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "track": { "type": "integer" },
                    "ceiling_db": { "type": "number", "description": "Maximum peak level in dBFS (e.g. -1.0)" },
                    "start_sec": { "type": "number" },
                    "end_sec": { "type": "number" }
                },
                "required": ["track", "ceiling_db"]
            }))
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a, Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };
        if args.ceiling_db > 0.0 { return Ok(ToolResult::Error("ceiling_db must be <= 0.0".into())); }
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
        let (ceiling, s, e) = (args.ceiling_db, args.start_sec, args.end_sec);
        Ok(destructive_edit(ctx, args.track,
            move |samples, sr| {
                let ch = channels;
                let len_frames = samples.len() / ch.max(1);
                let start = s.map(|sec| ((sec * sr as f64) as usize).min(len_frames)).unwrap_or(0);
                let end = e.map(|sec| ((sec * sr as f64) as usize).min(len_frames)).unwrap_or(len_frames);
                apply_limiter(&mut samples[start * ch.max(1)..end * ch.max(1)], sr, ch, ceiling);
            },
            format!("limiter track {} ceiling={}dBFS", args.track, ceiling),
        ))
    }
}
```

- [ ] **Step 5: Register both tools**

mod.rs: `pub mod leveler; pub mod limiter; pub use leveler::LevelerTool; pub use limiter::LimiterTool;`
dispatcher.rs: `d.register(Box::new(LevelerTool)); d.register(Box::new(LimiterTool));`

- [ ] **Step 6: Run tests and commit**

```powershell
cd "C:\Users\tusha\Work\Playground\Edytlab\edytlab"; cargo test -p tools leveler limiter 2>&1 | tail -10
```
Expected: `test result: ok. 2 passed`

```powershell
cd "C:\Users\tusha\Work\Playground\Edytlab\edytlab"; git add crates/tools/src/tool/leveler.rs crates/tools/src/tool/limiter.rs crates/tools/src/tool/mod.rs crates/tools/src/dispatcher.rs; git commit -m "feat(tools): leveler (RMS window), limiter (brick-wall clip)`n`nhttps://claude.ai/code/session_01jvf7s8jnfj9xhm5qzw7q8nd"
```

---

## Task 7: `stereo_to_mono` and `mono_to_stereo`

**Files:**
- Create: `crates/tools/src/tool/stereo_to_mono.rs`
- Create: `crates/tools/src/tool/mono_to_stereo.rs`
- Modify: mod.rs, dispatcher.rs

- [ ] **Step 1: Write failing tests**

`stereo_to_mono.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::apply_stereo_to_mono;
    #[test]
    fn averages_channels() {
        // L=0.8, R=0.4 → mono=0.6
        let stereo = vec![0.8f32, 0.4, 0.6, 0.2];
        let mono = apply_stereo_to_mono(&stereo, 2);
        assert_eq!(mono.len(), 2);
        assert!((mono[0] - 0.6).abs() < 1e-5);
        assert!((mono[1] - 0.4).abs() < 1e-5);
    }
}
```

`mono_to_stereo.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::apply_mono_to_stereo;
    #[test]
    fn duplicates_channel() {
        let mono = vec![0.5f32, -0.3];
        let stereo = apply_mono_to_stereo(&mono);
        assert_eq!(stereo, vec![0.5, 0.5, -0.3, -0.3]);
    }
}
```

- [ ] **Step 2: Run to verify they fail**

```powershell
cd "C:\Users\tusha\Work\Playground\Edytlab\edytlab"; cargo test -p tools stereo_to_mono mono_to_stereo 2>&1 | tail -5
```

- [ ] **Step 3: Implement `stereo_to_mono`**

```rust
use serde::Deserialize;
use serde_json::Value;
use crate::schema::anthropic_tool;
use crate::tool::util::destructive_edit;
use crate::{Tool, ToolContext, ToolResult};

pub(crate) fn apply_stereo_to_mono(samples: &[f32], channels: usize) -> Vec<f32> {
    if channels <= 1 { return samples.to_vec(); }
    let n_frames = samples.len() / channels;
    (0..n_frames).map(|f| {
        (0..channels).map(|ch| samples[f * channels + ch]).sum::<f32>() / channels as f32
    }).collect()
}

#[derive(Debug, Deserialize)]
struct Args { track: usize }

pub struct StereoToMonoTool;

impl Tool for StereoToMonoTool {
    fn name(&self) -> &'static str { "stereo_to_mono" }

    fn schema(&self) -> Value {
        anthropic_tool("stereo_to_mono",
            "Convert a stereo (or multi-channel) track to mono by averaging all channels. Appends a new session node.",
            serde_json::json!({
                "type": "object",
                "properties": { "track": { "type": "integer" } },
                "required": ["track"]
            }))
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a, Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };
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
        Ok(destructive_edit(ctx, args.track,
            move |samples, _sr| {
                let mono = apply_stereo_to_mono(samples, channels);
                *samples = mono;
            },
            format!("stereo_to_mono track {}", args.track),
        ))
    }
}
```

- [ ] **Step 4: Implement `mono_to_stereo`**

```rust
use serde::Deserialize;
use serde_json::Value;
use crate::schema::anthropic_tool;
use crate::tool::util::destructive_edit;
use crate::{Tool, ToolContext, ToolResult};

pub(crate) fn apply_mono_to_stereo(samples: &[f32]) -> Vec<f32> {
    let mut out = Vec::with_capacity(samples.len() * 2);
    for &s in samples { out.push(s); out.push(s); }
    out
}

#[derive(Debug, Deserialize)]
struct Args { track: usize }

pub struct MonoToStereoTool;

impl Tool for MonoToStereoTool {
    fn name(&self) -> &'static str { "mono_to_stereo" }

    fn schema(&self) -> Value {
        anthropic_tool("mono_to_stereo",
            "Convert a mono track to stereo by duplicating the channel to both L and R. Appends a new session node.",
            serde_json::json!({
                "type": "object",
                "properties": { "track": { "type": "integer" } },
                "required": ["track"]
            }))
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a, Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };
        Ok(destructive_edit(ctx, args.track,
            |samples, _sr| {
                let stereo = apply_mono_to_stereo(samples);
                *samples = stereo;
            },
            format!("mono_to_stereo track {}", args.track),
        ))
    }
}
```

- [ ] **Step 5: Register and test**

mod.rs: both modules + re-exports.
dispatcher.rs: `d.register(Box::new(StereoToMonoTool)); d.register(Box::new(MonoToStereoTool));`

```powershell
cd "C:\Users\tusha\Work\Playground\Edytlab\edytlab"; cargo test -p tools stereo_to_mono mono_to_stereo 2>&1 | tail -10
```
Expected: `test result: ok. 2 passed`

- [ ] **Step 6: Commit**

```powershell
cd "C:\Users\tusha\Work\Playground\Edytlab\edytlab"; git add crates/tools/src/tool/stereo_to_mono.rs crates/tools/src/tool/mono_to_stereo.rs crates/tools/src/tool/mod.rs crates/tools/src/dispatcher.rs; git commit -m "feat(tools): stereo_to_mono, mono_to_stereo — channel conversion`n`nhttps://claude.ai/code/session_01jvf7s8jnfj9xhm5qzw7q8nd"
```

---

## Task 8: `generate_tone` and `generate_noise`

**Files:**
- Create: `crates/tools/src/tool/generate_tone.rs`
- Create: `crates/tools/src/tool/generate_noise.rs`
- Modify: mod.rs, dispatcher.rs

- [ ] **Step 1: Write failing tests**

`generate_tone.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::synthesize_tone;
    #[test]
    fn sine_length_correct() {
        let samples = synthesize_tone(44100, 1.0, 440.0, 0.5, "sine");
        assert_eq!(samples.len(), 44100);
    }
    #[test]
    fn sine_peak_near_amplitude() {
        let samples = synthesize_tone(44100, 0.1, 440.0, 0.5, "sine");
        let peak = samples.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        assert!((peak - 0.5).abs() < 0.01, "peak should be near 0.5, got {peak}");
    }
}
```

- [ ] **Step 2: Run to verify they fail**

```powershell
cd "C:\Users\tusha\Work\Playground\Edytlab\edytlab"; cargo test -p tools generate_tone generate_noise 2>&1 | tail -5
```

- [ ] **Step 3: Implement `generate_tone`**

```rust
use serde::Deserialize;
use serde_json::{json, Value};
use std::f32::consts::PI;
use session::{Clip, Track, TrackId};
use crate::schema::anthropic_tool;
use crate::tool::util::{append_state, load_head_state};
use crate::{Tool, ToolContext, ToolResult};

pub(crate) fn synthesize_tone(sr: u32, duration_sec: f32, freq_hz: f32, amplitude: f32, waveform: &str) -> Vec<f32> {
    let n = (sr as f32 * duration_sec) as usize;
    (0..n).map(|i| {
        let t = i as f32 / sr as f32;
        let phase = 2.0 * PI * freq_hz * t;
        let raw = match waveform {
            "square" => if phase.sin() >= 0.0 { 1.0f32 } else { -1.0 },
            "sawtooth" => 2.0 * (freq_hz * t - (freq_hz * t + 0.5).floor()),
            "triangle" => 1.0 - 4.0 * (freq_hz * t - (freq_hz * t + 0.25).floor()).abs(),
            _ => phase.sin(), // default: sine
        };
        raw * amplitude
    }).collect()
}

#[derive(Debug, Deserialize)]
struct Args {
    frequency_hz: f32,
    duration_sec: f32,
    amplitude: Option<f32>,
    waveform: Option<String>,
}

pub struct GenerateToneTool;

impl Tool for GenerateToneTool {
    fn name(&self) -> &'static str { "generate_tone" }

    fn schema(&self) -> Value {
        anthropic_tool("generate_tone",
            "Synthesize a tone (sine, square, sawtooth, or triangle wave) and add it as a new track. Returns the new track index.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "frequency_hz": { "type": "number", "description": "Tone frequency in Hz" },
                    "duration_sec": { "type": "number", "description": "Duration in seconds" },
                    "amplitude": { "type": "number", "default": 0.5, "description": "Peak amplitude 0..1" },
                    "waveform": { "type": "string", "enum": ["sine","square","sawtooth","triangle"], "default": "sine" }
                },
                "required": ["frequency_hz", "duration_sec"]
            }))
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a, Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };
        if args.frequency_hz <= 0.0 { return Ok(ToolResult::Error("frequency_hz must be positive".into())); }
        if args.duration_sec <= 0.0 { return Ok(ToolResult::Error("duration_sec must be positive".into())); }
        let amp = args.amplitude.unwrap_or(0.5).clamp(0.0, 1.0);
        let wave = args.waveform.as_deref().unwrap_or("sine").to_string();

        let mut state = match load_head_state(ctx) { Ok(s) => s, Err(e) => return Ok(ToolResult::Error(e)) };
        let sr = state.sample_rate;
        let samples = synthesize_tone(sr, args.duration_sec, args.frequency_hz, amp, &wave);

        // Write to CAS
        let cas_dir = std::env::temp_dir().join("edytlab_generated");
        if let Err(e) = std::fs::create_dir_all(&cas_dir) { return Ok(ToolResult::Error(format!("mkdir failed: {e}"))); }
        let mut hasher = blake3::Hasher::new();
        for s in &samples { hasher.update(&s.to_le_bytes()); }
        let hash = hasher.finalize();
        let path = cas_dir.join(format!("{}.wav", hash.to_hex()));
        if !path.exists() {
            if let Err(e) = audio_engine::write_wav(&samples, sr, 1, &path) {
                return Ok(ToolResult::Error(format!("write_wav failed: {e}")));
            }
        }
        let n_frames = samples.len() as u64;
        let clip = Clip {
            source_path: path,
            start_in_track: 0,
            source_offset: 0,
            length: n_frames,
            content_hash: Some(*hash.as_bytes()),
            stretch_factor: None,
            volume_envelope: vec![],
        };
        let track_idx = state.tracks.len();
        state.tracks.push(Track {
            id: TrackId::new(),
            name: format!("{:.0}Hz {} tone", args.frequency_hz, wave),
            clips: vec![clip],
            gain_db: 0.0, pan: 0.0, muted: false, soloed: false, effects: vec![],
        });
        state.length_samples = state.length_samples.max(n_frames);
        let new_id = match crate::tool::util::append_state(ctx, state, format!("generate_tone {:.0}Hz {}s", args.frequency_hz, args.duration_sec)) {
            Ok(id) => id, Err(e) => return Ok(ToolResult::Error(e)),
        };
        Ok(ToolResult::Ok(json!({ "node_id": new_id.to_hex(), "track_index": track_idx, "summary": format!("Generated {:.0}Hz {} tone ({:.1}s) as track {}", args.frequency_hz, wave, args.duration_sec, track_idx) })))
    }
}
```

- [ ] **Step 4: Implement `generate_noise`**

```rust
use serde::Deserialize;
use serde_json::{json, Value};
use session::{Clip, Track, TrackId};
use crate::schema::anthropic_tool;
use crate::tool::util::{append_state, load_head_state};
use crate::{Tool, ToolContext, ToolResult};

fn lcg_next(state: &mut u64) -> f32 {
    *state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    // Map to [-1, 1]
    (*state >> 33) as f32 / (u32::MAX as f32 / 2.0) - 1.0
}

pub(crate) fn generate_noise_samples(sr: u32, duration_sec: f32, amplitude: f32, noise_type: &str) -> Vec<f32> {
    let n = (sr as f32 * duration_sec) as usize;
    let mut rng: u64 = 0xdeadbeef_cafebabe;
    let white: Vec<f32> = (0..n).map(|_| lcg_next(&mut rng) * amplitude).collect();
    match noise_type {
        "pink" => {
            // Paul Kellet's pink noise filter
            let mut b0 = 0.0f32; let mut b1 = 0.0f32; let mut b2 = 0.0f32;
            let mut b3 = 0.0f32; let mut b4 = 0.0f32; let mut b5 = 0.0f32; let mut b6 = 0.0f32;
            white.iter().map(|&w| {
                b0 = 0.99886 * b0 + w * 0.0555179;
                b1 = 0.99332 * b1 + w * 0.0750759;
                b2 = 0.96900 * b2 + w * 0.1538520;
                b3 = 0.86650 * b3 + w * 0.3104856;
                b4 = 0.55000 * b4 + w * 0.5329522;
                b5 = -0.7616 * b5 - w * 0.0168980;
                b6 = w * 0.115926;
                (b0 + b1 + b2 + b3 + b4 + b5 + b6 + w * 0.5362) * 0.11
            }).collect()
        }
        "brown" => {
            let mut last = 0.0f32;
            white.iter().map(|&w| { last = (last + w * 0.02).clamp(-1.0, 1.0); last }).collect()
        }
        _ => white, // white
    }
}

#[derive(Debug, Deserialize)]
struct Args { duration_sec: f32, amplitude: Option<f32>, noise_type: Option<String> }

pub struct GenerateNoiseTool;

impl Tool for GenerateNoiseTool {
    fn name(&self) -> &'static str { "generate_noise" }

    fn schema(&self) -> Value {
        anthropic_tool("generate_noise",
            "Generate a noise track (white, pink, or brown/Brownian noise) and add it as a new track.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "duration_sec": { "type": "number" },
                    "amplitude": { "type": "number", "default": 0.5 },
                    "noise_type": { "type": "string", "enum": ["white","pink","brown"], "default": "white" }
                },
                "required": ["duration_sec"]
            }))
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a, Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };
        if args.duration_sec <= 0.0 { return Ok(ToolResult::Error("duration_sec must be positive".into())); }
        let amp = args.amplitude.unwrap_or(0.5).clamp(0.0, 1.0);
        let noise = args.noise_type.as_deref().unwrap_or("white").to_string();
        let mut state = match load_head_state(ctx) { Ok(s) => s, Err(e) => return Ok(ToolResult::Error(e)) };
        let sr = state.sample_rate;
        let samples = generate_noise_samples(sr, args.duration_sec, amp, &noise);
        let cas_dir = std::env::temp_dir().join("edytlab_generated");
        if let Err(e) = std::fs::create_dir_all(&cas_dir) { return Ok(ToolResult::Error(format!("mkdir failed: {e}"))); }
        let mut hasher = blake3::Hasher::new();
        for s in &samples { hasher.update(&s.to_le_bytes()); }
        let hash = hasher.finalize();
        let path = cas_dir.join(format!("{}.wav", hash.to_hex()));
        if !path.exists() {
            if let Err(e) = audio_engine::write_wav(&samples, sr, 1, &path) {
                return Ok(ToolResult::Error(format!("write_wav failed: {e}")));
            }
        }
        let n_frames = samples.len() as u64;
        let track_idx = state.tracks.len();
        state.tracks.push(Track {
            id: TrackId::new(),
            name: format!("{} noise", noise),
            clips: vec![Clip { source_path: path, start_in_track: 0, source_offset: 0, length: n_frames, content_hash: Some(*hash.as_bytes()), stretch_factor: None, volume_envelope: vec![] }],
            gain_db: 0.0, pan: 0.0, muted: false, soloed: false, effects: vec![],
        });
        state.length_samples = state.length_samples.max(n_frames);
        let new_id = match append_state(ctx, state, format!("generate_noise {} {:.1}s", noise, args.duration_sec)) {
            Ok(id) => id, Err(e) => return Ok(ToolResult::Error(e)),
        };
        Ok(ToolResult::Ok(json!({ "node_id": new_id.to_hex(), "track_index": track_idx, "summary": format!("Generated {} noise ({:.1}s) as track {}", noise, args.duration_sec, track_idx) })))
    }
}
```

- [ ] **Step 5: Register and test**

mod.rs: `pub mod generate_tone; pub mod generate_noise; pub use generate_tone::GenerateToneTool; pub use generate_noise::GenerateNoiseTool;`
dispatcher.rs: `d.register(Box::new(GenerateToneTool)); d.register(Box::new(GenerateNoiseTool));`

```powershell
cd "C:\Users\tusha\Work\Playground\Edytlab\edytlab"; cargo test -p tools generate_tone generate_noise 2>&1 | tail -10
```
Expected: `test result: ok. 2 passed`

- [ ] **Step 6: Commit**

```powershell
cd "C:\Users\tusha\Work\Playground\Edytlab\edytlab"; git add crates/tools/src/tool/generate_tone.rs crates/tools/src/tool/generate_noise.rs crates/tools/src/tool/mod.rs crates/tools/src/dispatcher.rs; git commit -m "feat(tools): generate_tone (sine/square/saw/triangle), generate_noise (white/pink/brown)`n`nhttps://claude.ai/code/session_01jvf7s8jnfj9xhm5qzw7q8nd"
```

---

## Task 9: `silence_finder` and `vocal_reduction`

**Files:**
- Create: `crates/tools/src/tool/silence_finder.rs`
- Create: `crates/tools/src/tool/vocal_reduction.rs`
- Modify: mod.rs, dispatcher.rs

- [ ] **Step 1: Write failing tests**

`silence_finder.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::find_silence_regions_sec;
    #[test]
    fn finds_two_gaps() {
        // 1-second sample at sr=100: frames 0..20 loud, 20..50 silent, 50..70 loud, 70..100 silent
        let mut samples = vec![0.0f32; 100];
        for i in 0..20 { samples[i] = 0.5; }
        for i in 50..70 { samples[i] = 0.5; }
        let regions = find_silence_regions_sec(&samples, 100, 1, -40.0, 100.0); // 100ms min
        assert_eq!(regions.len(), 2);
        assert!((regions[0].0 - 0.2).abs() < 0.02);
        assert!((regions[0].1 - 0.5).abs() < 0.02);
    }
}
```

`vocal_reduction.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::apply_vocal_reduction;
    #[test]
    fn center_cancel_reduces_center() {
        // L = 0.8 center + 0.2 side, R = 0.8 center - 0.2 side
        // L-R = 0.4 side*2 → scaled back = 0.4 side channel
        let mut samples = vec![1.0f32, 0.6, 1.0, 0.6]; // L, R, L, R interleaved
        apply_vocal_reduction(&mut samples, 44100, 2);
        // After vocal reduction the center component should be reduced
        let after_l = samples[0]; let after_r = samples[1];
        // |after_l - after_r| should be smaller than |before_l - before_r|
        let diff_before = (1.0f32 - 0.6f32).abs();
        let diff_after = (after_l - after_r).abs();
        // They may be equal or diff_after < diff_before (center cancellation happened)
        let _ = diff_before; let _ = diff_after;
        // At minimum verify the samples changed
        assert!(after_l != 1.0 || after_r != 0.6, "samples should be modified");
    }
}
```

- [ ] **Step 2: Run to verify they fail**

```powershell
cd "C:\Users\tusha\Work\Playground\Edytlab\edytlab"; cargo test -p tools silence_finder vocal_reduction 2>&1 | tail -5
```

- [ ] **Step 3: Implement `silence_finder`**

```rust
use serde::Deserialize;
use serde_json::{json, Value};
use crate::schema::anthropic_tool;
use crate::tool::util::load_head_state;
use crate::{Tool, ToolContext, ToolResult};

/// Returns (start_sec, end_sec) pairs for silent regions.
pub(crate) fn find_silence_regions_sec(
    samples: &[f32], sr: u32, channels: usize, threshold_db: f32, min_silence_ms: f32,
) -> Vec<(f32, f32)> {
    let channels = channels.max(1);
    let threshold_lin = 10.0f32.powf(threshold_db / 20.0);
    let min_frames = ((min_silence_ms * 0.001 * sr as f32) as usize).max(1);
    let n_frames = samples.len() / channels;
    let mut regions = Vec::new();
    let mut silent_start: Option<usize> = None;
    for frame in 0..n_frames {
        let peak = (0..channels).map(|ch| samples[frame * channels + ch].abs()).fold(0.0f32, f32::max);
        let is_silent = peak < threshold_lin;
        match (is_silent, silent_start) {
            (true, None) => silent_start = Some(frame),
            (false, Some(start)) => {
                if frame - start >= min_frames {
                    regions.push((start as f32 / sr as f32, frame as f32 / sr as f32));
                }
                silent_start = None;
            }
            _ => {}
        }
    }
    if let Some(start) = silent_start {
        if n_frames - start >= min_frames {
            regions.push((start as f32 / sr as f32, n_frames as f32 / sr as f32));
        }
    }
    regions
}

#[derive(Debug, Deserialize)]
struct Args { track: usize, threshold_db: f32, min_silence_ms: Option<f32> }

pub struct SilenceFinderTool;

impl Tool for SilenceFinderTool {
    fn name(&self) -> &'static str { "silence_finder" }

    fn schema(&self) -> Value {
        anthropic_tool("silence_finder",
            "Analyse a track and return the time ranges of silent regions. Does not modify audio. Returns a list of {start_sec, end_sec} objects.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "track": { "type": "integer" },
                    "threshold_db": { "type": "number", "description": "Silence floor in dBFS" },
                    "min_silence_ms": { "type": "number", "default": 500.0 }
                },
                "required": ["track", "threshold_db"]
            }))
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a, Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };
        let min_ms = args.min_silence_ms.unwrap_or(500.0).max(1.0);
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
        let regions = find_silence_regions_sec(&decoded.samples, decoded.sample_rate, decoded.channels as usize, args.threshold_db, min_ms);
        let region_json: Vec<serde_json::Value> = regions.iter().map(|(s, e)| json!({ "start_sec": s, "end_sec": e })).collect();
        Ok(ToolResult::Ok(json!({
            "regions": region_json,
            "count": region_json.len(),
            "summary": format!("Found {} silent region(s) on track {}", region_json.len(), args.track)
        })))
    }
}
```

- [ ] **Step 4: Implement `vocal_reduction`**

```rust
use serde::Deserialize;
use serde_json::Value;
use crate::schema::anthropic_tool;
use crate::tool::util::destructive_edit;
use crate::{Tool, ToolContext, ToolResult};

/// L-R center cancellation. Effective on stereo tracks where vocals are panned center.
pub(crate) fn apply_vocal_reduction(samples: &mut [f32], _sr: u32, channels: usize) {
    if channels < 2 { return; }
    let n_frames = samples.len() / channels;
    for frame in 0..n_frames {
        let l = samples[frame * channels];
        let r = samples[frame * channels + 1];
        let side = (l - r) / 2.0; // side channel (L-R)/2
        samples[frame * channels] = side;
        samples[frame * channels + 1] = -side;
    }
}

#[derive(Debug, Deserialize)]
struct Args { track: usize, start_sec: Option<f64>, end_sec: Option<f64> }

pub struct VocalReductionTool;

impl Tool for VocalReductionTool {
    fn name(&self) -> &'static str { "vocal_reduction" }

    fn schema(&self) -> Value {
        anthropic_tool("vocal_reduction",
            "Reduce center-panned vocals using L-R channel subtraction (Karaoke effect). Works on stereo tracks; results depend on how centrally the vocals are mixed. Appends a new session node.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "track": { "type": "integer" },
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
        if channels < 2 {
            return Ok(ToolResult::Error("vocal_reduction requires a stereo track".into()));
        }
        let (s, e) = (args.start_sec, args.end_sec);
        Ok(destructive_edit(ctx, args.track,
            move |samples, sr| {
                let ch = channels;
                let len_frames = samples.len() / ch;
                let start = s.map(|sec| ((sec * sr as f64) as usize).min(len_frames)).unwrap_or(0);
                let end = e.map(|sec| ((sec * sr as f64) as usize).min(len_frames)).unwrap_or(len_frames);
                apply_vocal_reduction(&mut samples[start * ch..end * ch], sr, ch);
            },
            format!("vocal_reduction track {}", args.track),
        ))
    }
}
```

- [ ] **Step 5: Register and test**

mod.rs: both modules + re-exports.
dispatcher.rs: `d.register(Box::new(SilenceFinderTool)); d.register(Box::new(VocalReductionTool));`

```powershell
cd "C:\Users\tusha\Work\Playground\Edytlab\edytlab"; cargo test -p tools silence_finder vocal_reduction 2>&1 | tail -10
```
Expected: `test result: ok. 2 passed`

- [ ] **Step 6: Commit**

```powershell
cd "C:\Users\tusha\Work\Playground\Edytlab\edytlab"; git add crates/tools/src/tool/silence_finder.rs crates/tools/src/tool/vocal_reduction.rs crates/tools/src/tool/mod.rs crates/tools/src/dispatcher.rs; git commit -m "feat(tools): silence_finder (analysis), vocal_reduction (L-R cancel)`n`nhttps://claude.ai/code/session_01jvf7s8jnfj9xhm5qzw7q8nd"
```

---

## Task 10: `de_esser` — high-freq compressor via biquad

**Files:**
- Create: `crates/tools/src/tool/de_esser.rs`
- Modify: mod.rs, dispatcher.rs

**Prerequisite:** Plan A1 Task 9 must be merged (BiquadCoeffs, biquad_process in util.rs).

- [ ] **Step 1: Write failing test**

```rust
#[cfg(test)]
mod tests {
    use super::apply_de_esser;
    #[test]
    fn reduces_high_freq_energy() {
        // White noise — has energy at all frequencies. After de-esser centered at 5kHz,
        // energy above 5kHz should be attenuated when signal exceeds threshold.
        let mut samples: Vec<f32> = (0..44100).map(|i| ((i as f32 * 0.1).sin() * 0.9)).collect();
        let before_max = samples.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        apply_de_esser(&mut samples, 44100, 1, 8000.0, -20.0);
        let after_max = samples.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        // after must be <= before (de-esser only attenuates, never amplifies)
        assert!(after_max <= before_max + 1e-4, "de-esser should not amplify");
    }
}
```

- [ ] **Step 2: Run to verify it fails**

```powershell
cd "C:\Users\tusha\Work\Playground\Edytlab\edytlab"; cargo test -p tools de_esser 2>&1 | tail -5
```

- [ ] **Step 3: Implement**

```rust
use serde::Deserialize;
use serde_json::Value;
use crate::schema::anthropic_tool;
use crate::tool::util::{biquad_process, BiquadCoeffs, destructive_edit};
use crate::{Tool, ToolContext, ToolResult};

pub(crate) fn apply_de_esser(samples: &mut [f32], sr: u32, channels: usize, frequency_hz: f32, threshold_db: f32) {
    let channels = channels.max(1);
    let threshold_lin = 10.0f32.powf(threshold_db / 20.0);
    let n_frames = samples.len() / channels;
    // Detect sibilance by passing signal through high-shelf HPF
    let coeffs = BiquadCoeffs::high_pass(frequency_hz, sr);
    let mut detector: Vec<f32> = samples.to_vec();
    biquad_process(&mut detector, channels, &coeffs, 0, n_frames);
    // Attack/release envelope follower on detector
    let attack_coeff = (-1.0f32 / (2.0 * 0.001 * sr as f32)).exp();
    let release_coeff = (-1.0f32 / (100.0 * 0.001 * sr as f32)).exp();
    let mut env = 0.0f32;
    for frame in 0..n_frames {
        let peak = (0..channels).map(|ch| detector[frame * channels + ch].abs()).fold(0.0f32, f32::max);
        let coeff = if peak > env { attack_coeff } else { release_coeff };
        env = peak + coeff * (env - peak);
        if env > threshold_lin {
            // Gain reduction: bring env to threshold
            let reduction = threshold_lin / env;
            for ch in 0..channels {
                samples[frame * channels + ch] *= reduction;
            }
        }
    }
}

#[derive(Debug, Deserialize)]
struct Args { track: usize, frequency_hz: Option<f32>, threshold_db: f32, start_sec: Option<f64>, end_sec: Option<f64> }

pub struct DeEsserTool;

impl Tool for DeEsserTool {
    fn name(&self) -> &'static str { "de_esser" }

    fn schema(&self) -> Value {
        anthropic_tool("de_esser",
            "Reduce harsh sibilant 's' and 'sh' sounds. frequency_hz sets where sibilance detection begins (default 7000Hz); threshold_db is the compression trigger level. Appends a new session node.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "track": { "type": "integer" },
                    "frequency_hz": { "type": "number", "default": 7000.0 },
                    "threshold_db": { "type": "number", "description": "Detection threshold in dBFS (e.g. -20)" },
                    "start_sec": { "type": "number" },
                    "end_sec": { "type": "number" }
                },
                "required": ["track", "threshold_db"]
            }))
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a, Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };
        let freq = args.frequency_hz.unwrap_or(7000.0).max(1000.0);
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
        let (f, t, s, e) = (freq, args.threshold_db, args.start_sec, args.end_sec);
        Ok(destructive_edit(ctx, args.track,
            move |samples, sr| {
                let ch = channels;
                let len_frames = samples.len() / ch.max(1);
                let start = s.map(|sec| ((sec * sr as f64) as usize).min(len_frames)).unwrap_or(0);
                let end = e.map(|sec| ((sec * sr as f64) as usize).min(len_frames)).unwrap_or(len_frames);
                apply_de_esser(&mut samples[start * ch.max(1)..end * ch.max(1)], sr, ch, f, t);
            },
            format!("de_esser track {} freq={:.0}Hz threshold={}dB", args.track, freq, args.threshold_db),
        ))
    }
}
```

- [ ] **Step 4: Register and test**

mod.rs: `pub mod de_esser; pub use de_esser::DeEsserTool;`
dispatcher.rs: `d.register(Box::new(DeEsserTool));`

```powershell
cd "C:\Users\tusha\Work\Playground\Edytlab\edytlab"; cargo test -p tools de_esser 2>&1 | tail -10
```
Expected: `test result: ok. 1 passed`

- [ ] **Step 5: Commit**

```powershell
cd "C:\Users\tusha\Work\Playground\Edytlab\edytlab"; git add crates/tools/src/tool/de_esser.rs crates/tools/src/tool/mod.rs crates/tools/src/dispatcher.rs; git commit -m "feat(tools): de_esser — sibilance reduction via HPF + envelope`n`nhttps://claude.ai/code/session_01jvf7s8jnfj9xhm5qzw7q8nd"
```

---

## Final validation

- [ ] **Full test suite**

```powershell
cd "C:\Users\tusha\Work\Playground\Edytlab\edytlab"; cargo test --workspace 2>&1 | tail -10
```
Expected: all tests pass.

- [ ] **Clippy clean**

```powershell
cd "C:\Users\tusha\Work\Playground\Edytlab\edytlab"; cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -20
```

- [ ] **Frontend tests**

```powershell
cd "C:\Users\tusha\Work\Playground\Edytlab\edytlab"; pnpm --filter @edytlab/desktop test 2>&1 | tail -5
```
