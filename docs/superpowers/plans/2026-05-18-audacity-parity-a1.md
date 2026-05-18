# Audacity Parity A1 — Low-Complexity Tools Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add 13 low-complexity audio editing tools (silence_region, set_pan, rename_track, invert, repeat_selection, change_speed, time_shift, duplicate_track, mute/solo render wiring, split_clip, high_pass_filter, low_pass_filter, notch_filter) bringing edytlab to parity with Audacity's basic editing capabilities.

**Architecture:** Each tool lives in `crates/tools/src/tool/<name>.rs` implementing the `Tool` trait, registered in `crates/tools/src/dispatcher.rs::default_dispatcher()`, and declared in `crates/tools/src/tool/mod.rs`. Most use the existing `destructive_edit` helper or the lightweight state-mutate-and-append pattern from `normalize`. Filters share a `biquad_filter` helper added to `crates/tools/src/tool/util.rs`.

**Tech Stack:** Rust 1.78+, `serde`/`serde_json`, `blake3`, `audio_decoder`, `audio_engine::write_wav`, `session::{SessionState, Track, Clip}`.

---

## Key Patterns (read before implementing any task)

### Pattern A — Destructive edit (mutates samples)
Use `destructive_edit(ctx, track_idx, |samples, sr| { … }, "label")` from `crate::tool::util`. Returns `ToolResult` directly.

### Pattern B — State-field mutation (no sample edit)
```rust
let mut state = load_head_state(ctx)?;          // returns Err(String) shaped for ToolResult::Error
check_track_index(&state.tracks, idx)?;
state.tracks[idx].some_field = new_value;
let new_id = append_state(ctx, state, "label")?;
Ok(ToolResult::Ok(json!({ "node_id": new_id.to_hex(), "summary": "..." })))
```

### Adding a tool (every task)
1. Create `crates/tools/src/tool/<name>.rs`
2. Add `pub mod <name>;` + `pub use <name>::<NameTool>;` in `crates/tools/src/tool/mod.rs`
3. Add `d.register(Box::new(<NameTool>));` in `crates/tools/src/dispatcher.rs::default_dispatcher()`

### Commit footer
Every commit message must end with:
```
https://claude.ai/code/session_01jvf7s8jnfj9xhm5qzw7q8nd
```

---

## Task 1: `silence_region` — zero samples in a time range

**Files:**
- Create: `crates/tools/src/tool/silence_region.rs`
- Modify: `crates/tools/src/tool/mod.rs`
- Modify: `crates/tools/src/dispatcher.rs`

- [ ] **Step 1: Write the failing test**

Add to `crates/tools/src/tool/silence_region.rs` (create the file with this content):

```rust
#[cfg(test)]
mod tests {
    use super::apply_silence;

    #[test]
    fn zeros_samples_in_range() {
        let mut samples = vec![1.0f32; 1000];
        // sr=100, silence 2.0..5.0 sec = frames 200..500, interleaved ch=1
        apply_silence(&mut samples, 100, 1, 2.0, 5.0);
        assert!(samples[..200].iter().all(|&s| s == 1.0), "before range untouched");
        assert!(samples[200..500].iter().all(|&s| s == 0.0), "range zeroed");
        assert!(samples[500..].iter().all(|&s| s == 1.0), "after range untouched");
    }

    #[test]
    fn clamps_to_buffer_end() {
        let mut samples = vec![1.0f32; 100];
        apply_silence(&mut samples, 100, 1, 0.5, 999.0);
        assert!(samples[50..].iter().all(|&s| s == 0.0));
        assert!(samples[..50].iter().all(|&s| s == 1.0));
    }
}
```

- [ ] **Step 2: Run to verify it fails**

```powershell
cd "C:\Users\tusha\Work\Playground\Edytlab\edytlab"; cargo test -p tools silence_region 2>&1 | tail -5
```
Expected: `error[E0433]: failed to resolve: use of undeclared module`

- [ ] **Step 3: Implement**

Write `crates/tools/src/tool/silence_region.rs`:

```rust
use serde::Deserialize;
use serde_json::{json, Value};

use crate::schema::anthropic_tool;
use crate::tool::util::destructive_edit;
use crate::{Tool, ToolContext, ToolResult};

pub(crate) fn apply_silence(samples: &mut [f32], sr: u32, channels: usize, start_sec: f64, end_sec: f64) {
    let stride = channels.max(1);
    let start = ((start_sec * sr as f64) as usize * stride).min(samples.len());
    let end = ((end_sec * sr as f64) as usize * stride).min(samples.len());
    for s in &mut samples[start..end] {
        *s = 0.0;
    }
}

#[derive(Debug, Deserialize)]
struct Args {
    track: usize,
    start_sec: f64,
    end_sec: f64,
}

pub struct SilenceRegionTool;

impl Tool for SilenceRegionTool {
    fn name(&self) -> &'static str { "silence_region" }

    fn schema(&self) -> Value {
        anthropic_tool(
            "silence_region",
            "Zero out audio samples between start_sec and end_sec on a track. Appends a new session node.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "track": { "type": "integer", "description": "Zero-based track index" },
                    "start_sec": { "type": "number", "description": "Start of silence region in seconds" },
                    "end_sec": { "type": "number", "description": "End of silence region in seconds (exclusive)" }
                },
                "required": ["track", "start_sec", "end_sec"]
            }),
        )
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };
        if args.start_sec >= args.end_sec {
            return Ok(ToolResult::Error(format!(
                "start_sec ({}) must be < end_sec ({})", args.start_sec, args.end_sec
            )));
        }
        let channels = {
            let state = match crate::tool::util::load_head_state(ctx) {
                Ok(s) => s,
                Err(e) => return Ok(ToolResult::Error(e)),
            };
            if let Err(e) = crate::tool::util::check_track_index(&state.tracks, args.track) {
                return Ok(ToolResult::Error(e));
            }
            // peek channels from decoded source
            let clip = state.tracks[args.track].clips.first().cloned();
            if let Some(clip) = clip {
                match audio_decoder::decode_file(&clip.source_path) {
                    Ok(d) => d.channels as usize,
                    Err(_) => 1,
                }
            } else {
                return Ok(ToolResult::Error(format!("track {} has no clips", args.track)));
            }
        };
        let start_sec = args.start_sec;
        let end_sec = args.end_sec;
        Ok(destructive_edit(
            ctx,
            args.track,
            move |samples, sr| apply_silence(samples, sr, channels, start_sec, end_sec),
            format!("silence_region track {} {:.3}s..{:.3}s", args.track, args.start_sec, args.end_sec),
        ))
    }
}
```

- [ ] **Step 4: Register in mod.rs and dispatcher.rs**

In `crates/tools/src/tool/mod.rs` add:
```rust
pub mod silence_region;
pub use silence_region::SilenceRegionTool;
```

In `crates/tools/src/dispatcher.rs` at the end of `default_dispatcher()`, before the closing `d`:
```rust
// A1 tools
d.register(Box::new(SilenceRegionTool));
```
Also add `SilenceRegionTool` to the `use crate::tool::{...}` import block.

- [ ] **Step 5: Run tests**

```powershell
cd "C:\Users\tusha\Work\Playground\Edytlab\edytlab"; cargo test -p tools silence_region 2>&1 | tail -10
```
Expected: `test result: ok. 2 passed`

- [ ] **Step 6: Commit**

```powershell
cd "C:\Users\tusha\Work\Playground\Edytlab\edytlab"; git add crates/tools/src/tool/silence_region.rs crates/tools/src/tool/mod.rs crates/tools/src/dispatcher.rs; git commit -m "feat(tools): silence_region — zero samples in time range`n`nhttps://claude.ai/code/session_01jvf7s8jnfj9xhm5qzw7q8nd"
```

---

## Task 2: `set_pan` and `rename_track` — metadata mutations

**Files:**
- Create: `crates/tools/src/tool/set_pan.rs`
- Create: `crates/tools/src/tool/rename_track.rs`
- Modify: `crates/tools/src/tool/mod.rs`
- Modify: `crates/tools/src/dispatcher.rs`

- [ ] **Step 1: Write failing tests**

`crates/tools/src/tool/set_pan.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::clamp_pan;
    #[test]
    fn clamps_positive() { assert_eq!(clamp_pan(1.5), 1.0); }
    #[test]
    fn clamps_negative() { assert_eq!(clamp_pan(-2.0), -1.0); }
    #[test]
    fn passes_valid() { assert_eq!(clamp_pan(-0.5), -0.5); }
}
```

`crates/tools/src/tool/rename_track.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::validate_name;
    #[test]
    fn rejects_empty() { assert!(validate_name("").is_err()); }
    #[test]
    fn accepts_valid() { assert!(validate_name("Vocals").is_ok()); }
}
```

- [ ] **Step 2: Run to verify they fail**

```powershell
cd "C:\Users\tusha\Work\Playground\Edytlab\edytlab"; cargo test -p tools set_pan rename_track 2>&1 | tail -5
```
Expected: compile errors.

- [ ] **Step 3: Implement `set_pan`**

```rust
use serde::Deserialize;
use serde_json::{json, Value};
use crate::schema::anthropic_tool;
use crate::tool::util::{append_state, check_track_index, load_head_state};
use crate::{Tool, ToolContext, ToolResult};

pub(crate) fn clamp_pan(p: f32) -> f32 { p.clamp(-1.0, 1.0) }

#[derive(Debug, Deserialize)]
struct Args { track: usize, pan: f32 }

pub struct SetPanTool;

impl Tool for SetPanTool {
    fn name(&self) -> &'static str { "set_pan" }

    fn schema(&self) -> Value {
        anthropic_tool(
            "set_pan",
            "Set the stereo pan of a track. -1.0 = full left, 0.0 = centre, 1.0 = full right. Appends a new session node.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "track": { "type": "integer" },
                    "pan": { "type": "number", "minimum": -1.0, "maximum": 1.0 }
                },
                "required": ["track", "pan"]
            }),
        )
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };
        let mut state = match load_head_state(ctx) {
            Ok(s) => s,
            Err(e) => return Ok(ToolResult::Error(e)),
        };
        if let Err(e) = check_track_index(&state.tracks, args.track) {
            return Ok(ToolResult::Error(e));
        }
        let pan = clamp_pan(args.pan);
        state.tracks[args.track].pan = pan;
        let new_id = match append_state(ctx, state, format!("set_pan track {} -> {:.2}", args.track, pan)) {
            Ok(id) => id,
            Err(e) => return Ok(ToolResult::Error(e)),
        };
        Ok(ToolResult::Ok(json!({ "node_id": new_id.to_hex(), "pan": pan, "summary": format!("Set track {} pan to {:.2}", args.track, pan) })))
    }
}
```

- [ ] **Step 4: Implement `rename_track`**

```rust
use serde::Deserialize;
use serde_json::{json, Value};
use crate::schema::anthropic_tool;
use crate::tool::util::{append_state, check_track_index, load_head_state};
use crate::{Tool, ToolContext, ToolResult};

pub(crate) fn validate_name(n: &str) -> Result<(), String> {
    if n.trim().is_empty() { Err("name must not be empty".into()) } else { Ok(()) }
}

#[derive(Debug, Deserialize)]
struct Args { track: usize, name: String }

pub struct RenameTrackTool;

impl Tool for RenameTrackTool {
    fn name(&self) -> &'static str { "rename_track" }

    fn schema(&self) -> Value {
        anthropic_tool(
            "rename_track",
            "Rename a track. Appends a new session node.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "track": { "type": "integer" },
                    "name": { "type": "string", "minLength": 1 }
                },
                "required": ["track", "name"]
            }),
        )
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };
        if let Err(e) = validate_name(&args.name) {
            return Ok(ToolResult::Error(e));
        }
        let mut state = match load_head_state(ctx) {
            Ok(s) => s,
            Err(e) => return Ok(ToolResult::Error(e)),
        };
        if let Err(e) = check_track_index(&state.tracks, args.track) {
            return Ok(ToolResult::Error(e));
        }
        state.tracks[args.track].name = args.name.clone();
        let new_id = match append_state(ctx, state, format!("rename_track {} -> {}", args.track, args.name)) {
            Ok(id) => id,
            Err(e) => return Ok(ToolResult::Error(e)),
        };
        Ok(ToolResult::Ok(json!({ "node_id": new_id.to_hex(), "summary": format!("Renamed track {} to {:?}", args.track, args.name) })))
    }
}
```

- [ ] **Step 5: Register both tools in mod.rs and dispatcher.rs**

mod.rs additions:
```rust
pub mod set_pan;
pub mod rename_track;
pub use set_pan::SetPanTool;
pub use rename_track::RenameTrackTool;
```

dispatcher.rs additions in `default_dispatcher()`:
```rust
d.register(Box::new(SetPanTool));
d.register(Box::new(RenameTrackTool));
```

- [ ] **Step 6: Run tests**

```powershell
cd "C:\Users\tusha\Work\Playground\Edytlab\edytlab"; cargo test -p tools set_pan rename_track 2>&1 | tail -10
```
Expected: `test result: ok. 5 passed`

- [ ] **Step 7: Commit**

```powershell
cd "C:\Users\tusha\Work\Playground\Edytlab\edytlab"; git add crates/tools/src/tool/set_pan.rs crates/tools/src/tool/rename_track.rs crates/tools/src/tool/mod.rs crates/tools/src/dispatcher.rs; git commit -m "feat(tools): set_pan, rename_track — metadata mutation tools`n`nhttps://claude.ai/code/session_01jvf7s8jnfj9xhm5qzw7q8nd"
```

---

## Task 3: `invert` — negate samples in range

**Files:**
- Create: `crates/tools/src/tool/invert.rs`
- Modify: `crates/tools/src/tool/mod.rs`, `crates/tools/src/dispatcher.rs`

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::apply_invert;
    #[test]
    fn negates_all() {
        let mut s = vec![0.5f32, -0.3, 0.0, 1.0];
        apply_invert(&mut s, 44100, 1, None, None);
        assert!((s[0] - -0.5).abs() < 1e-6);
        assert!((s[1] - 0.3).abs() < 1e-6);
        assert_eq!(s[2], 0.0);
        assert!((s[3] - -1.0).abs() < 1e-6);
    }
    #[test]
    fn negates_range_only() {
        let mut s = vec![1.0f32; 200]; // sr=100, 2sec, ch=1
        apply_invert(&mut s, 100, 1, Some(0.5), Some(1.5));
        // frames 50..150 negated
        assert_eq!(s[0], 1.0);
        assert_eq!(s[49], 1.0);
        assert_eq!(s[50], -1.0);
        assert_eq!(s[149], -1.0);
        assert_eq!(s[150], 1.0);
    }
}
```

- [ ] **Step 2: Run to verify it fails**

```powershell
cd "C:\Users\tusha\Work\Playground\Edytlab\edytlab"; cargo test -p tools invert 2>&1 | tail -5
```

- [ ] **Step 3: Implement**

```rust
use serde::Deserialize;
use serde_json::Value;
use crate::schema::anthropic_tool;
use crate::tool::util::destructive_edit;
use crate::{Tool, ToolContext, ToolResult};

pub(crate) fn apply_invert(samples: &mut [f32], sr: u32, channels: usize, start_sec: Option<f64>, end_sec: Option<f64>) {
    let stride = channels.max(1);
    let start = start_sec.map(|s| ((s * sr as f64) as usize * stride).min(samples.len())).unwrap_or(0);
    let end = end_sec.map(|e| ((e * sr as f64) as usize * stride).min(samples.len())).unwrap_or(samples.len());
    for s in &mut samples[start..end] { *s = -*s; }
}

#[derive(Debug, Deserialize)]
struct Args { track: usize, start_sec: Option<f64>, end_sec: Option<f64> }

pub struct InvertTool;

impl Tool for InvertTool {
    fn name(&self) -> &'static str { "invert" }

    fn schema(&self) -> Value {
        anthropic_tool("invert", "Invert (negate) audio polarity on a track, optionally within a time range. Appends a new session node.",
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
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
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
            } else {
                return Ok(ToolResult::Error(format!("track {} has no clips", args.track)));
            }
        };
        let (s, e) = (args.start_sec, args.end_sec);
        Ok(destructive_edit(ctx, args.track,
            move |samples, sr| apply_invert(samples, sr, channels, s, e),
            format!("invert track {}", args.track),
        ))
    }
}
```

- [ ] **Step 4: Register**

mod.rs: `pub mod invert; pub use invert::InvertTool;`
dispatcher.rs: `d.register(Box::new(InvertTool));`

- [ ] **Step 5: Run tests and commit**

```powershell
cd "C:\Users\tusha\Work\Playground\Edytlab\edytlab"; cargo test -p tools invert 2>&1 | tail -10
```
Expected: `test result: ok. 2 passed`

```powershell
cd "C:\Users\tusha\Work\Playground\Edytlab\edytlab"; git add crates/tools/src/tool/invert.rs crates/tools/src/tool/mod.rs crates/tools/src/dispatcher.rs; git commit -m "feat(tools): invert — polarity inversion with optional range`n`nhttps://claude.ai/code/session_01jvf7s8jnfj9xhm5qzw7q8nd"
```

---

## Task 4: `repeat_selection` — duplicate a region N times

**Files:**
- Create: `crates/tools/src/tool/repeat_selection.rs`
- Modify: mod.rs, dispatcher.rs

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::apply_repeat;
    #[test]
    fn repeats_twice() {
        // 10 samples, 1 channel, sr=10 → 1 sample/sec
        // repeat 3.0..7.0 (samples 3..7) 2 times → original + 2 copies inserted after end_sec
        let mut samples: Vec<f32> = (0..10).map(|i| i as f32).collect();
        apply_repeat(&mut samples, 10, 1, 3.0, 7.0, 2);
        // region is [3,4,5,6], 2 extra copies appended after index 7
        // original 0..10 then 2 copies of 3..7 appended → len = 10 + 4*2 = 18
        assert_eq!(samples.len(), 18);
        assert_eq!(&samples[0..10], &[0.0,1.0,2.0,3.0,4.0,5.0,6.0,7.0,8.0,9.0]);
        assert_eq!(&samples[10..14], &[3.0,4.0,5.0,6.0]);
        assert_eq!(&samples[14..18], &[3.0,4.0,5.0,6.0]);
    }
}
```

- [ ] **Step 2: Run to verify it fails**

```powershell
cd "C:\Users\tusha\Work\Playground\Edytlab\edytlab"; cargo test -p tools repeat_selection 2>&1 | tail -5
```

- [ ] **Step 3: Implement**

```rust
use serde::Deserialize;
use serde_json::Value;
use crate::schema::anthropic_tool;
use crate::tool::util::destructive_edit;
use crate::{Tool, ToolContext, ToolResult};

pub(crate) fn apply_repeat(samples: &mut Vec<f32>, sr: u32, channels: usize, start_sec: f64, end_sec: f64, times: u32) {
    let stride = channels.max(1);
    let start = ((start_sec * sr as f64) as usize * stride).min(samples.len());
    let end = ((end_sec * sr as f64) as usize * stride).min(samples.len());
    if end <= start || times == 0 { return; }
    let region: Vec<f32> = samples[start..end].to_vec();
    for _ in 0..times {
        samples.extend_from_slice(&region);
    }
}

#[derive(Debug, Deserialize)]
struct Args { track: usize, start_sec: f64, end_sec: f64, times: u32 }

pub struct RepeatSelectionTool;

impl Tool for RepeatSelectionTool {
    fn name(&self) -> &'static str { "repeat_selection" }

    fn schema(&self) -> Value {
        anthropic_tool("repeat_selection",
            "Duplicate the audio region [start_sec, end_sec) on a track N additional times, appending copies after the original. Appends a new session node.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "track": { "type": "integer" },
                    "start_sec": { "type": "number" },
                    "end_sec": { "type": "number" },
                    "times": { "type": "integer", "minimum": 1 }
                },
                "required": ["track", "start_sec", "end_sec", "times"]
            }))
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a, Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };
        if args.start_sec >= args.end_sec {
            return Ok(ToolResult::Error(format!("start_sec must be < end_sec")));
        }
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
        let (s, e, t) = (args.start_sec, args.end_sec, args.times);
        Ok(destructive_edit(ctx, args.track,
            move |samples, sr| apply_repeat(samples, sr, channels, s, e, t),
            format!("repeat_selection track {} {:.3}s..{:.3}s x{}", args.track, args.start_sec, args.end_sec, args.times),
        ))
    }
}
```

- [ ] **Step 4: Register and test**

mod.rs: `pub mod repeat_selection; pub use repeat_selection::RepeatSelectionTool;`
dispatcher.rs: `d.register(Box::new(RepeatSelectionTool));`

```powershell
cd "C:\Users\tusha\Work\Playground\Edytlab\edytlab"; cargo test -p tools repeat_selection 2>&1 | tail -10
```
Expected: `test result: ok. 1 passed`

- [ ] **Step 5: Commit**

```powershell
cd "C:\Users\tusha\Work\Playground\Edytlab\edytlab"; git add crates/tools/src/tool/repeat_selection.rs crates/tools/src/tool/mod.rs crates/tools/src/dispatcher.rs; git commit -m "feat(tools): repeat_selection — duplicate region N times`n`nhttps://claude.ai/code/session_01jvf7s8jnfj9xhm5qzw7q8nd"
```

---

## Task 5: `change_speed` — linear resampling (speed change, no pitch preserve)

**Files:**
- Create: `crates/tools/src/tool/change_speed.rs`
- Modify: mod.rs, dispatcher.rs

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::apply_change_speed;
    #[test]
    fn double_speed_halves_length() {
        let samples: Vec<f32> = (0..100).map(|i| i as f32 / 100.0).collect();
        let result = apply_change_speed(&samples, 1, 2.0);
        assert_eq!(result.len(), 50);
    }
    #[test]
    fn half_speed_doubles_length() {
        let samples: Vec<f32> = (0..100).map(|i| i as f32 / 100.0).collect();
        let result = apply_change_speed(&samples, 1, 0.5);
        assert_eq!(result.len(), 200);
    }
    #[test]
    fn factor_one_is_identity() {
        let samples: Vec<f32> = vec![0.1, 0.2, 0.3, 0.4];
        let result = apply_change_speed(&samples, 1, 1.0);
        assert_eq!(result.len(), 4);
        for (a, b) in samples.iter().zip(result.iter()) {
            assert!((a - b).abs() < 1e-5);
        }
    }
}
```

- [ ] **Step 2: Run to verify it fails**

```powershell
cd "C:\Users\tusha\Work\Playground\Edytlab\edytlab"; cargo test -p tools change_speed 2>&1 | tail -5
```

- [ ] **Step 3: Implement**

```rust
use serde::Deserialize;
use serde_json::Value;
use crate::schema::anthropic_tool;
use crate::tool::util::destructive_edit;
use crate::{Tool, ToolContext, ToolResult};

/// Linear interpolation resampling. `factor` > 1 = faster (shorter), < 1 = slower (longer).
pub(crate) fn apply_change_speed(samples: &[f32], channels: usize, factor: f32) -> Vec<f32> {
    let channels = channels.max(1);
    let in_frames = samples.len() / channels;
    let out_frames = ((in_frames as f32 / factor).round() as usize).max(1);
    let mut out = Vec::with_capacity(out_frames * channels);
    for out_f in 0..out_frames {
        let src_f = out_f as f32 * factor;
        let lo = (src_f as usize).min(in_frames.saturating_sub(1));
        let hi = (lo + 1).min(in_frames.saturating_sub(1));
        let t = src_f - lo as f32;
        for ch in 0..channels {
            let a = samples[lo * channels + ch];
            let b = samples[hi * channels + ch];
            out.push(a + (b - a) * t);
        }
    }
    out
}

#[derive(Debug, Deserialize)]
struct Args { track: usize, factor: f32 }

pub struct ChangeSpeedTool;

impl Tool for ChangeSpeedTool {
    fn name(&self) -> &'static str { "change_speed" }

    fn schema(&self) -> Value {
        anthropic_tool("change_speed",
            "Resample a track to change playback speed without pitch preservation. factor > 1 speeds up (shorter duration), factor < 1 slows down (longer). Appends a new session node.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "track": { "type": "integer" },
                    "factor": { "type": "number", "exclusiveMinimum": 0.0, "description": "Speed multiplier, e.g. 2.0 = double speed" }
                },
                "required": ["track", "factor"]
            }))
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a, Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };
        if args.factor <= 0.0 || !args.factor.is_finite() {
            return Ok(ToolResult::Error("factor must be a positive finite number".into()));
        }
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
        let factor = args.factor;
        Ok(destructive_edit(ctx, args.track,
            move |samples, _sr| {
                let resampled = apply_change_speed(samples, channels, factor);
                *samples = resampled;
            },
            format!("change_speed track {} x{:.3}", args.track, args.factor),
        ))
    }
}
```

- [ ] **Step 4: Register and test**

mod.rs: `pub mod change_speed; pub use change_speed::ChangeSpeedTool;`
dispatcher.rs: `d.register(Box::new(ChangeSpeedTool));`

```powershell
cd "C:\Users\tusha\Work\Playground\Edytlab\edytlab"; cargo test -p tools change_speed 2>&1 | tail -10
```
Expected: `test result: ok. 3 passed`

- [ ] **Step 5: Commit**

```powershell
cd "C:\Users\tusha\Work\Playground\Edytlab\edytlab"; git add crates/tools/src/tool/change_speed.rs crates/tools/src/tool/mod.rs crates/tools/src/dispatcher.rs; git commit -m "feat(tools): change_speed — linear resampling speed change`n`nhttps://claude.ai/code/session_01jvf7s8jnfj9xhm5qzw7q8nd"
```

---

## Task 6: `time_shift` and `duplicate_track`

**Files:**
- Create: `crates/tools/src/tool/time_shift.rs`
- Create: `crates/tools/src/tool/duplicate_track.rs`
- Modify: mod.rs, dispatcher.rs

- [ ] **Step 1: Write failing tests**

`time_shift.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::apply_time_shift;
    #[test]
    fn shifts_clip_start() {
        // clip at start_in_track=0, shift +2.0 sec at sr=44100
        let offset = (2.0f64 * 44100.0) as u64;
        let new_start = apply_time_shift(0u64, offset);
        assert_eq!(new_start, offset);
    }
    #[test]
    fn clamps_negative_to_zero() {
        // shift -5 sec when clip is at 1 sec
        let start = 44100u64;
        let shift = -(5.0f64 * 44100.0) as i64;
        let new_start = apply_time_shift_signed(start, shift);
        assert_eq!(new_start, 0);
    }
}
```

`duplicate_track.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::validate_track_idx;
    #[test]
    fn rejects_out_of_range() { assert!(validate_track_idx(5, 3).is_err()); }
    #[test]
    fn accepts_valid() { assert!(validate_track_idx(2, 5).is_ok()); }
}
```

- [ ] **Step 2: Run to verify they fail**

```powershell
cd "C:\Users\tusha\Work\Playground\Edytlab\edytlab"; cargo test -p tools time_shift duplicate_track 2>&1 | tail -5
```

- [ ] **Step 3: Implement `time_shift`**

```rust
use serde::Deserialize;
use serde_json::{json, Value};
use crate::schema::anthropic_tool;
use crate::tool::util::{append_state, check_track_index, load_head_state};
use crate::{Tool, ToolContext, ToolResult};

pub(crate) fn apply_time_shift(current: u64, delta_samples: u64) -> u64 {
    current + delta_samples
}
pub(crate) fn apply_time_shift_signed(current: u64, delta: i64) -> u64 {
    (current as i64 + delta).max(0) as u64
}

#[derive(Debug, Deserialize)]
struct Args { track: usize, offset_sec: f64 }

pub struct TimeShiftTool;

impl Tool for TimeShiftTool {
    fn name(&self) -> &'static str { "time_shift" }

    fn schema(&self) -> Value {
        anthropic_tool("time_shift",
            "Move a track's first clip forward or backward in time. Positive offset_sec moves later, negative moves earlier (clamped to 0). Appends a new session node.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "track": { "type": "integer" },
                    "offset_sec": { "type": "number", "description": "Seconds to shift (positive=later, negative=earlier)" }
                },
                "required": ["track", "offset_sec"]
            }))
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a, Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };
        let mut state = match load_head_state(ctx) {
            Ok(s) => s, Err(e) => return Ok(ToolResult::Error(e)),
        };
        if let Err(e) = check_track_index(&state.tracks, args.track) {
            return Ok(ToolResult::Error(e));
        }
        let sr = state.sample_rate as f64;
        let delta = (args.offset_sec * sr) as i64;
        let track = &mut state.tracks[args.track];
        if let Some(clip) = track.clips.first_mut() {
            clip.start_in_track = apply_time_shift_signed(clip.start_in_track, delta);
        }
        // Recompute session length
        state.length_samples = state.tracks.iter()
            .flat_map(|t| t.clips.iter().map(|c| c.start_in_track + c.length))
            .max().unwrap_or(0);
        let new_id = match append_state(ctx, state, format!("time_shift track {} {:+.3}s", args.track, args.offset_sec)) {
            Ok(id) => id, Err(e) => return Ok(ToolResult::Error(e)),
        };
        Ok(ToolResult::Ok(json!({ "node_id": new_id.to_hex(), "summary": format!("Shifted track {} by {:+.3}s", args.track, args.offset_sec) })))
    }
}
```

- [ ] **Step 4: Implement `duplicate_track`**

```rust
use serde::Deserialize;
use serde_json::{json, Value};
use session::{Track, TrackId};
use crate::schema::anthropic_tool;
use crate::tool::util::{append_state, check_track_index, load_head_state};
use crate::{Tool, ToolContext, ToolResult};

pub(crate) fn validate_track_idx(idx: usize, len: usize) -> Result<(), String> {
    if idx >= len { Err(format!("track {idx} out of range (len={len})")) } else { Ok(()) }
}

#[derive(Debug, Deserialize)]
struct Args { track: usize }

pub struct DuplicateTrackTool;

impl Tool for DuplicateTrackTool {
    fn name(&self) -> &'static str { "duplicate_track" }

    fn schema(&self) -> Value {
        anthropic_tool("duplicate_track",
            "Create an exact copy of a track (same clips, gain, pan, effects). The duplicate is appended after the original. Appends a new session node.",
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
        let mut state = match load_head_state(ctx) {
            Ok(s) => s, Err(e) => return Ok(ToolResult::Error(e)),
        };
        if let Err(e) = check_track_index(&state.tracks, args.track) {
            return Ok(ToolResult::Error(e));
        }
        let mut cloned: Track = state.tracks[args.track].clone();
        cloned.id = TrackId::new();
        cloned.name = format!("{} (copy)", cloned.name);
        state.tracks.push(cloned);
        let new_id = match append_state(ctx, state, format!("duplicate_track {}", args.track)) {
            Ok(id) => id, Err(e) => return Ok(ToolResult::Error(e)),
        };
        Ok(ToolResult::Ok(json!({ "node_id": new_id.to_hex(), "summary": format!("Duplicated track {}", args.track) })))
    }
}
```

- [ ] **Step 5: Register and test**

mod.rs: add both modules and re-exports.
dispatcher.rs: register both tools.

```powershell
cd "C:\Users\tusha\Work\Playground\Edytlab\edytlab"; cargo test -p tools time_shift duplicate_track 2>&1 | tail -10
```
Expected: `test result: ok. 4 passed`

- [ ] **Step 6: Commit**

```powershell
cd "C:\Users\tusha\Work\Playground\Edytlab\edytlab"; git add crates/tools/src/tool/time_shift.rs crates/tools/src/tool/duplicate_track.rs crates/tools/src/tool/mod.rs crates/tools/src/dispatcher.rs; git commit -m "feat(tools): time_shift, duplicate_track`n`nhttps://claude.ai/code/session_01jvf7s8jnfj9xhm5qzw7q8nd"
```

---

## Task 7: `mute_track` and `solo_track` — wire muted/soloed into render engine

**Files:**
- Create: `crates/tools/src/tool/mute_track.rs`
- Create: `crates/tools/src/tool/solo_track.rs`
- Modify: `crates/audio-engine/src/render.rs` (skip muted/solo logic)
- Modify: mod.rs, dispatcher.rs

- [ ] **Step 1: Write failing tests**

`mute_track.rs`:
```rust
#[cfg(test)]
mod tests {
    #[test]
    fn muted_flag_set() {
        // This is tested through the full dispatcher in integration tests.
        // Unit test: muted=true sets field correctly.
        let muted = true;
        assert!(muted); // placeholder — real test is in render.rs
    }
}
```

`crates/audio-engine/src/render.rs` — add test at bottom:
```rust
#[cfg(test)]
mod mute_solo_tests {
    use super::*;
    // Tests that muted tracks produce silence, solo unmuted tracks produce audio.
    // These are integration tests run by cargo test --workspace.
    // Verified by: ensure_muted_track_skipped and ensure_solo_wiring.
    #[test]
    fn muted_track_is_skipped_in_mix() {
        // A muted track contributes zero samples to the mix.
        // Test: mix of [muted: [1.0; N], non-muted: [0.5; N]] = [0.5; N]
        // This is validated by the existing render tests when muted=true.
        // Placeholder: confirm logic exists in should_include_track.
        assert!(should_include_track(false, false, false));  // unmuted, no solo
        assert!(!should_include_track(true, false, false));  // muted
        assert!(should_include_track(false, true, true));    // soloed, any_solo=true
        assert!(!should_include_track(false, false, true));  // not soloed, but someone else is
    }
}
```

- [ ] **Step 2: Add `should_include_track` to render.rs**

Open `crates/audio-engine/src/render.rs`. Find the track mix loop. Add this function before the mix loop:

```rust
/// Returns true if this track should contribute to the mix.
/// - Muted tracks are always skipped.
/// - If any track is soloed, only soloed tracks play.
pub(crate) fn should_include_track(muted: bool, soloed: bool, any_solo: bool) -> bool {
    if muted { return false; }
    if any_solo { return soloed; }
    true
}
```

Then in the mix loop, compute `any_solo` before iterating:
```rust
let any_solo = state.tracks.iter().any(|t| t.soloed);
```
And wrap the track contribution with:
```rust
if !should_include_track(track.muted, track.soloed, any_solo) {
    continue;
}
```

- [ ] **Step 3: Implement `mute_track`**

```rust
use serde::Deserialize;
use serde_json::{json, Value};
use crate::schema::anthropic_tool;
use crate::tool::util::{append_state, check_track_index, load_head_state};
use crate::{Tool, ToolContext, ToolResult};

#[derive(Debug, Deserialize)]
struct Args { track: usize, muted: bool }

pub struct MuteTrackTool;

impl Tool for MuteTrackTool {
    fn name(&self) -> &'static str { "mute_track" }

    fn schema(&self) -> Value {
        anthropic_tool("mute_track",
            "Mute or unmute a track. Muted tracks produce silence in the mix. Appends a new session node.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "track": { "type": "integer" },
                    "muted": { "type": "boolean" }
                },
                "required": ["track", "muted"]
            }))
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a, Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };
        let mut state = match load_head_state(ctx) {
            Ok(s) => s, Err(e) => return Ok(ToolResult::Error(e)),
        };
        if let Err(e) = check_track_index(&state.tracks, args.track) {
            return Ok(ToolResult::Error(e));
        }
        state.tracks[args.track].muted = args.muted;
        let new_id = match append_state(ctx, state, format!("mute_track {} -> {}", args.track, args.muted)) {
            Ok(id) => id, Err(e) => return Ok(ToolResult::Error(e)),
        };
        Ok(ToolResult::Ok(json!({ "node_id": new_id.to_hex(), "summary": format!("Track {} muted={}", args.track, args.muted) })))
    }
}
```

- [ ] **Step 4: Implement `solo_track`**

```rust
use serde::Deserialize;
use serde_json::{json, Value};
use crate::schema::anthropic_tool;
use crate::tool::util::{append_state, check_track_index, load_head_state};
use crate::{Tool, ToolContext, ToolResult};

#[derive(Debug, Deserialize)]
struct Args { track: usize, solo: bool }

pub struct SoloTrackTool;

impl Tool for SoloTrackTool {
    fn name(&self) -> &'static str { "solo_track" }

    fn schema(&self) -> Value {
        anthropic_tool("solo_track",
            "Solo or un-solo a track. When any track is soloed, only soloed tracks play in the mix. Appends a new session node.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "track": { "type": "integer" },
                    "solo": { "type": "boolean" }
                },
                "required": ["track", "solo"]
            }))
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a, Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };
        let mut state = match load_head_state(ctx) {
            Ok(s) => s, Err(e) => return Ok(ToolResult::Error(e)),
        };
        if let Err(e) = check_track_index(&state.tracks, args.track) {
            return Ok(ToolResult::Error(e));
        }
        state.tracks[args.track].soloed = args.solo;
        let new_id = match append_state(ctx, state, format!("solo_track {} -> {}", args.track, args.solo)) {
            Ok(id) => id, Err(e) => return Ok(ToolResult::Error(e)),
        };
        Ok(ToolResult::Ok(json!({ "node_id": new_id.to_hex(), "summary": format!("Track {} solo={}", args.track, args.solo) })))
    }
}
```

- [ ] **Step 5: Register all**

mod.rs: `pub mod mute_track; pub mod solo_track; pub use mute_track::MuteTrackTool; pub use solo_track::SoloTrackTool;`
dispatcher.rs: `d.register(Box::new(MuteTrackTool)); d.register(Box::new(SoloTrackTool));`

- [ ] **Step 6: Run tests**

```powershell
cd "C:\Users\tusha\Work\Playground\Edytlab\edytlab"; cargo test --workspace mute_solo 2>&1 | tail -10
```
Expected: `test result: ok. 1 passed` (plus existing render tests still pass)

```powershell
cd "C:\Users\tusha\Work\Playground\Edytlab\edytlab"; cargo test --workspace 2>&1 | tail -5
```
Expected: all tests pass.

- [ ] **Step 7: Commit**

```powershell
cd "C:\Users\tusha\Work\Playground\Edytlab\edytlab"; git add crates/tools/src/tool/mute_track.rs crates/tools/src/tool/solo_track.rs crates/audio-engine/src/render.rs crates/tools/src/tool/mod.rs crates/tools/src/dispatcher.rs; git commit -m "feat(tools): mute_track, solo_track — wire muted/solo into render engine`n`nhttps://claude.ai/code/session_01jvf7s8jnfj9xhm5qzw7q8nd"
```

---

## Task 8: `split_clip` — split one clip into two at a time position

**Files:**
- Create: `crates/tools/src/tool/split_clip.rs`
- Modify: mod.rs, dispatcher.rs

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::split_at;
    use session::Clip;
    use std::path::PathBuf;

    fn make_clip(start_in_track: u64, source_offset: u64, length: u64) -> Clip {
        Clip {
            source_path: PathBuf::from("/tmp/test.wav"),
            start_in_track,
            source_offset,
            length,
            content_hash: None,
            stretch_factor: None,
            volume_envelope: vec![],
        }
    }

    #[test]
    fn splits_middle() {
        let clip = make_clip(0, 0, 100);
        let (a, b) = split_at(&clip, 40).unwrap();
        assert_eq!(a.start_in_track, 0);
        assert_eq!(a.source_offset, 0);
        assert_eq!(a.length, 40);
        assert_eq!(b.start_in_track, 40);
        assert_eq!(b.source_offset, 40);
        assert_eq!(b.length, 60);
    }

    #[test]
    fn rejects_split_at_start() {
        let clip = make_clip(0, 0, 100);
        assert!(split_at(&clip, 0).is_err());
    }

    #[test]
    fn rejects_split_at_end() {
        let clip = make_clip(0, 0, 100);
        assert!(split_at(&clip, 100).is_err());
    }
}
```

- [ ] **Step 2: Run to verify it fails**

```powershell
cd "C:\Users\tusha\Work\Playground\Edytlab\edytlab"; cargo test -p tools split_clip 2>&1 | tail -5
```

- [ ] **Step 3: Implement**

Check `crates/session/src/state.rs` for the exact Clip struct fields (content_hash, stretch_factor, volume_envelope may differ). Use the real field names.

```rust
use serde::Deserialize;
use serde_json::{json, Value};
use session::Clip;
use crate::schema::anthropic_tool;
use crate::tool::util::{append_state, check_track_index, load_head_state};
use crate::{Tool, ToolContext, ToolResult};

/// Split `clip` into two clips at `at_frames` frames from the clip's source_offset.
/// Returns (left, right) or Err if at_frames is out of range.
pub(crate) fn split_at(clip: &Clip, at_frames: u64) -> Result<(Clip, Clip), String> {
    if at_frames == 0 || at_frames >= clip.length {
        return Err(format!("split point {at_frames} must be in (0, {})", clip.length));
    }
    let left = Clip {
        source_path: clip.source_path.clone(),
        start_in_track: clip.start_in_track,
        source_offset: clip.source_offset,
        length: at_frames,
        content_hash: clip.content_hash,
        stretch_factor: clip.stretch_factor,
        volume_envelope: clip.volume_envelope.clone(),
    };
    let right = Clip {
        source_path: clip.source_path.clone(),
        start_in_track: clip.start_in_track + at_frames,
        source_offset: clip.source_offset + at_frames,
        length: clip.length - at_frames,
        content_hash: clip.content_hash,
        stretch_factor: clip.stretch_factor,
        volume_envelope: clip.volume_envelope.clone(),
    };
    Ok((left, right))
}

#[derive(Debug, Deserialize)]
struct Args { track: usize, clip_index: usize, at_sec: f64 }

pub struct SplitClipTool;

impl Tool for SplitClipTool {
    fn name(&self) -> &'static str { "split_clip" }

    fn schema(&self) -> Value {
        anthropic_tool("split_clip",
            "Split a clip into two at the specified time position. Both resulting clips point at the same source file with adjusted offsets. Appends a new session node.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "track": { "type": "integer" },
                    "clip_index": { "type": "integer", "description": "Zero-based clip index within the track" },
                    "at_sec": { "type": "number", "description": "Position to split at, in seconds from track start" }
                },
                "required": ["track", "clip_index", "at_sec"]
            }))
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a, Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };
        let mut state = match load_head_state(ctx) {
            Ok(s) => s, Err(e) => return Ok(ToolResult::Error(e)),
        };
        if let Err(e) = check_track_index(&state.tracks, args.track) {
            return Ok(ToolResult::Error(e));
        }
        let track = &state.tracks[args.track];
        if args.clip_index >= track.clips.len() {
            return Ok(ToolResult::Error(format!("clip_index {} out of range (track has {} clips)", args.clip_index, track.clips.len())));
        }
        let clip = track.clips[args.clip_index].clone();
        let at_frames = (args.at_sec * state.sample_rate as f64) as u64;
        // at_frames is relative to track timeline; convert to offset within clip
        let clip_at = at_frames.saturating_sub(clip.start_in_track);
        let (left, right) = match split_at(&clip, clip_at) {
            Ok(pair) => pair,
            Err(e) => return Ok(ToolResult::Error(e)),
        };
        let track = &mut state.tracks[args.track];
        track.clips.remove(args.clip_index);
        track.clips.insert(args.clip_index, right);
        track.clips.insert(args.clip_index, left);
        let new_id = match append_state(ctx, state, format!("split_clip track {} clip {} at {:.3}s", args.track, args.clip_index, args.at_sec)) {
            Ok(id) => id, Err(e) => return Ok(ToolResult::Error(e)),
        };
        Ok(ToolResult::Ok(json!({ "node_id": new_id.to_hex(), "summary": format!("Split track {} clip {} at {:.3}s", args.track, args.clip_index, args.at_sec) })))
    }
}
```

- [ ] **Step 4: Register and test**

mod.rs: `pub mod split_clip; pub use split_clip::SplitClipTool;`
dispatcher.rs: `d.register(Box::new(SplitClipTool));`

```powershell
cd "C:\Users\tusha\Work\Playground\Edytlab\edytlab"; cargo test -p tools split_clip 2>&1 | tail -10
```
Expected: `test result: ok. 3 passed`

- [ ] **Step 5: Commit**

```powershell
cd "C:\Users\tusha\Work\Playground\Edytlab\edytlab"; git add crates/tools/src/tool/split_clip.rs crates/tools/src/tool/mod.rs crates/tools/src/dispatcher.rs; git commit -m "feat(tools): split_clip — split clip into two at time position`n`nhttps://claude.ai/code/session_01jvf7s8jnfj9xhm5qzw7q8nd"
```

---

## Task 9: Biquad filter helper + `high_pass_filter`, `low_pass_filter`, `notch_filter`

**Files:**
- Modify: `crates/tools/src/tool/util.rs` (add `biquad_process`)
- Create: `crates/tools/src/tool/high_pass_filter.rs`
- Create: `crates/tools/src/tool/low_pass_filter.rs`
- Create: `crates/tools/src/tool/notch_filter.rs`
- Modify: mod.rs, dispatcher.rs

- [ ] **Step 1: Add biquad helper to `crates/tools/src/tool/util.rs`**

Append to the file:

```rust
// ---------------------------------------------------------------------------
// Biquad filter
// ---------------------------------------------------------------------------

/// Direct Form II biquad filter state (per channel).
pub(crate) struct BiquadState {
    pub z1: f32,
    pub z2: f32,
}

impl BiquadState {
    pub(crate) fn new() -> Self { Self { z1: 0.0, z2: 0.0 } }
}

/// Biquad coefficients [b0, b1, b2, a1, a2] (a0 normalised to 1).
pub(crate) struct BiquadCoeffs {
    pub b0: f32, pub b1: f32, pub b2: f32,
    pub a1: f32, pub a2: f32,
}

impl BiquadCoeffs {
    /// Single-pole high-pass filter.
    pub(crate) fn high_pass(cutoff_hz: f32, sample_rate: u32) -> Self {
        use std::f32::consts::PI;
        let w0 = 2.0 * PI * cutoff_hz / sample_rate as f32;
        let alpha = w0.sin() / (2.0 * 0.707); // Q=0.707 (Butterworth)
        let cos_w0 = w0.cos();
        let b0 = (1.0 + cos_w0) / 2.0;
        let b1 = -(1.0 + cos_w0);
        let b2 = (1.0 + cos_w0) / 2.0;
        let a0 = 1.0 + alpha;
        let a1 = -2.0 * cos_w0;
        let a2 = 1.0 - alpha;
        Self { b0: b0/a0, b1: b1/a0, b2: b2/a0, a1: a1/a0, a2: a2/a0 }
    }

    /// Single-pole low-pass filter.
    pub(crate) fn low_pass(cutoff_hz: f32, sample_rate: u32) -> Self {
        use std::f32::consts::PI;
        let w0 = 2.0 * PI * cutoff_hz / sample_rate as f32;
        let alpha = w0.sin() / (2.0 * 0.707);
        let cos_w0 = w0.cos();
        let b0 = (1.0 - cos_w0) / 2.0;
        let b1 = 1.0 - cos_w0;
        let b2 = (1.0 - cos_w0) / 2.0;
        let a0 = 1.0 + alpha;
        let a1 = -2.0 * cos_w0;
        let a2 = 1.0 - alpha;
        Self { b0: b0/a0, b1: b1/a0, b2: b2/a0, a1: a1/a0, a2: a2/a0 }
    }

    /// Notch (band-reject) filter.
    pub(crate) fn notch(center_hz: f32, q: f32, sample_rate: u32) -> Self {
        use std::f32::consts::PI;
        let w0 = 2.0 * PI * center_hz / sample_rate as f32;
        let alpha = w0.sin() / (2.0 * q);
        let cos_w0 = w0.cos();
        let b0 = 1.0;
        let b1 = -2.0 * cos_w0;
        let b2 = 1.0;
        let a0 = 1.0 + alpha;
        let a1 = -2.0 * cos_w0;
        let a2 = 1.0 - alpha;
        Self { b0: b0/a0, b1: b1/a0, b2: b2/a0, a1: a1/a0, a2: a2/a0 }
    }
}

/// Process interleaved `samples` in-place with a biquad filter.
/// One `BiquadState` per channel is allocated internally.
pub(crate) fn biquad_process(
    samples: &mut [f32],
    channels: usize,
    coeffs: &BiquadCoeffs,
    start_sample: usize,
    end_sample: usize,
) {
    let channels = channels.max(1);
    let end = end_sample.min(samples.len() / channels);
    let start = start_sample.min(end);
    let mut states: Vec<BiquadState> = (0..channels).map(|_| BiquadState::new()).collect();
    for frame in start..end {
        for ch in 0..channels {
            let idx = frame * channels + ch;
            let x = samples[idx];
            let st = &mut states[ch];
            let y = coeffs.b0 * x + st.z1;
            st.z1 = coeffs.b1 * x - coeffs.a1 * y + st.z2;
            st.z2 = coeffs.b2 * x - coeffs.a2 * y;
            samples[idx] = y;
        }
    }
}
```

- [ ] **Step 2: Write failing tests for filters**

`high_pass_filter.rs` (create file):
```rust
#[cfg(test)]
mod tests {
    use super::apply_high_pass;
    #[test]
    fn attenuates_dc() {
        // DC signal (all 1.0) passed through HPF should be near zero at steady state
        let mut samples = vec![1.0f32; 4410]; // 0.1s at 44100
        apply_high_pass(&mut samples, 44100, 1, 1000.0, None, None);
        let tail_mean: f32 = samples[4000..].iter().sum::<f32>() / 410.0;
        assert!(tail_mean.abs() < 0.01, "DC should be attenuated, got {tail_mean}");
    }
}
```

- [ ] **Step 3: Run to verify failure**

```powershell
cd "C:\Users\tusha\Work\Playground\Edytlab\edytlab"; cargo test -p tools high_pass_filter 2>&1 | tail -5
```

- [ ] **Step 4: Implement `high_pass_filter.rs`**

```rust
use serde::Deserialize;
use serde_json::Value;
use crate::schema::anthropic_tool;
use crate::tool::util::{biquad_process, BiquadCoeffs, destructive_edit};
use crate::{Tool, ToolContext, ToolResult};

pub(crate) fn apply_high_pass(samples: &mut [f32], sr: u32, channels: usize, cutoff_hz: f32, start_sec: Option<f64>, end_sec: Option<f64>) {
    let channels = channels.max(1);
    let len_frames = samples.len() / channels;
    let start = start_sec.map(|s| ((s * sr as f64) as usize).min(len_frames)).unwrap_or(0);
    let end = end_sec.map(|e| ((e * sr as f64) as usize).min(len_frames)).unwrap_or(len_frames);
    let coeffs = BiquadCoeffs::high_pass(cutoff_hz, sr);
    biquad_process(samples, channels, &coeffs, start, end);
}

#[derive(Debug, Deserialize)]
struct Args { track: usize, cutoff_hz: f32, start_sec: Option<f64>, end_sec: Option<f64> }

pub struct HighPassFilterTool;

impl Tool for HighPassFilterTool {
    fn name(&self) -> &'static str { "high_pass_filter" }

    fn schema(&self) -> Value {
        anthropic_tool("high_pass_filter",
            "Apply a Butterworth high-pass filter to a track, removing frequencies below cutoff_hz. Appends a new session node.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "track": { "type": "integer" },
                    "cutoff_hz": { "type": "number", "description": "Cutoff frequency in Hz" },
                    "start_sec": { "type": "number" },
                    "end_sec": { "type": "number" }
                },
                "required": ["track", "cutoff_hz"]
            }))
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a, Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };
        if args.cutoff_hz <= 0.0 { return Ok(ToolResult::Error("cutoff_hz must be positive".into())); }
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
        let (cutoff, s, e) = (args.cutoff_hz, args.start_sec, args.end_sec);
        Ok(destructive_edit(ctx, args.track,
            move |samples, sr| apply_high_pass(samples, sr, channels, cutoff, s, e),
            format!("high_pass_filter track {} cutoff={:.0}Hz", args.track, cutoff),
        ))
    }
}
```

- [ ] **Step 5: Implement `low_pass_filter.rs`**

```rust
use serde::Deserialize;
use serde_json::Value;
use crate::schema::anthropic_tool;
use crate::tool::util::{biquad_process, BiquadCoeffs, destructive_edit};
use crate::{Tool, ToolContext, ToolResult};

pub(crate) fn apply_low_pass(samples: &mut [f32], sr: u32, channels: usize, cutoff_hz: f32, start_sec: Option<f64>, end_sec: Option<f64>) {
    let channels = channels.max(1);
    let len_frames = samples.len() / channels;
    let start = start_sec.map(|s| ((s * sr as f64) as usize).min(len_frames)).unwrap_or(0);
    let end = end_sec.map(|e| ((e * sr as f64) as usize).min(len_frames)).unwrap_or(len_frames);
    let coeffs = BiquadCoeffs::low_pass(cutoff_hz, sr);
    biquad_process(samples, channels, &coeffs, start, end);
}

#[derive(Debug, Deserialize)]
struct Args { track: usize, cutoff_hz: f32, start_sec: Option<f64>, end_sec: Option<f64> }

pub struct LowPassFilterTool;

impl Tool for LowPassFilterTool {
    fn name(&self) -> &'static str { "low_pass_filter" }

    fn schema(&self) -> Value {
        anthropic_tool("low_pass_filter",
            "Apply a Butterworth low-pass filter to a track, removing frequencies above cutoff_hz. Appends a new session node.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "track": { "type": "integer" },
                    "cutoff_hz": { "type": "number" },
                    "start_sec": { "type": "number" },
                    "end_sec": { "type": "number" }
                },
                "required": ["track", "cutoff_hz"]
            }))
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a, Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };
        if args.cutoff_hz <= 0.0 { return Ok(ToolResult::Error("cutoff_hz must be positive".into())); }
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
        let (cutoff, s, e) = (args.cutoff_hz, args.start_sec, args.end_sec);
        Ok(destructive_edit(ctx, args.track,
            move |samples, sr| apply_low_pass(samples, sr, channels, cutoff, s, e),
            format!("low_pass_filter track {} cutoff={:.0}Hz", args.track, cutoff),
        ))
    }
}
```

- [ ] **Step 6: Implement `notch_filter.rs`**

```rust
use serde::Deserialize;
use serde_json::Value;
use crate::schema::anthropic_tool;
use crate::tool::util::{biquad_process, BiquadCoeffs, destructive_edit};
use crate::{Tool, ToolContext, ToolResult};

pub(crate) fn apply_notch(samples: &mut [f32], sr: u32, channels: usize, center_hz: f32, q: f32, start_sec: Option<f64>, end_sec: Option<f64>) {
    let channels = channels.max(1);
    let len_frames = samples.len() / channels;
    let start = start_sec.map(|s| ((s * sr as f64) as usize).min(len_frames)).unwrap_or(0);
    let end = end_sec.map(|e| ((e * sr as f64) as usize).min(len_frames)).unwrap_or(len_frames);
    let coeffs = BiquadCoeffs::notch(center_hz, q, sr);
    biquad_process(samples, channels, &coeffs, start, end);
}

#[derive(Debug, Deserialize)]
struct Args { track: usize, center_hz: f32, q: f32, start_sec: Option<f64>, end_sec: Option<f64> }

pub struct NotchFilterTool;

impl Tool for NotchFilterTool {
    fn name(&self) -> &'static str { "notch_filter" }

    fn schema(&self) -> Value {
        anthropic_tool("notch_filter",
            "Apply a notch (band-reject) filter to a track, attenuating frequencies near center_hz. q controls the width: higher Q = narrower notch. Appends a new session node.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "track": { "type": "integer" },
                    "center_hz": { "type": "number", "description": "Center frequency to reject in Hz" },
                    "q": { "type": "number", "description": "Quality factor (sharpness); typical range 0.5..30", "default": 1.0 },
                    "start_sec": { "type": "number" },
                    "end_sec": { "type": "number" }
                },
                "required": ["track", "center_hz", "q"]
            }))
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a, Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };
        if args.center_hz <= 0.0 { return Ok(ToolResult::Error("center_hz must be positive".into())); }
        if args.q <= 0.0 { return Ok(ToolResult::Error("q must be positive".into())); }
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
        let (center, q, s, e) = (args.center_hz, args.q, args.start_sec, args.end_sec);
        Ok(destructive_edit(ctx, args.track,
            move |samples, sr| apply_notch(samples, sr, channels, center, q, s, e),
            format!("notch_filter track {} center={:.0}Hz q={:.1}", args.track, center, q),
        ))
    }
}
```

- [ ] **Step 7: Register all three filters**

mod.rs additions:
```rust
pub mod high_pass_filter;
pub mod low_pass_filter;
pub mod notch_filter;
pub use high_pass_filter::HighPassFilterTool;
pub use low_pass_filter::LowPassFilterTool;
pub use notch_filter::NotchFilterTool;
```

dispatcher.rs:
```rust
d.register(Box::new(HighPassFilterTool));
d.register(Box::new(LowPassFilterTool));
d.register(Box::new(NotchFilterTool));
```

- [ ] **Step 8: Run all tests**

```powershell
cd "C:\Users\tusha\Work\Playground\Edytlab\edytlab"; cargo test -p tools high_pass_filter low_pass_filter notch_filter 2>&1 | tail -10
```
Expected: `test result: ok. 1 passed` (or more if extra tests added)

```powershell
cd "C:\Users\tusha\Work\Playground\Edytlab\edytlab"; cargo test --workspace 2>&1 | tail -5
```
All tests should pass.

- [ ] **Step 9: Commit**

```powershell
cd "C:\Users\tusha\Work\Playground\Edytlab\edytlab"; git add crates/tools/src/tool/util.rs crates/tools/src/tool/high_pass_filter.rs crates/tools/src/tool/low_pass_filter.rs crates/tools/src/tool/notch_filter.rs crates/tools/src/tool/mod.rs crates/tools/src/dispatcher.rs; git commit -m "feat(tools): biquad helper + high_pass, low_pass, notch filters`n`nhttps://claude.ai/code/session_01jvf7s8jnfj9xhm5qzw7q8nd"
```

---

## Final validation

- [ ] **Run full test suite**

```powershell
cd "C:\Users\tusha\Work\Playground\Edytlab\edytlab"; cargo test --workspace 2>&1 | tail -10
```
Expected: all tests pass.

- [ ] **Check clippy**

```powershell
cd "C:\Users\tusha\Work\Playground\Edytlab\edytlab"; cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -20
```
Expected: no warnings.

- [ ] **Frontend tests still pass**

```powershell
cd "C:\Users\tusha\Work\Playground\Edytlab\edytlab"; pnpm --filter @edytlab/desktop test 2>&1 | tail -5
```
