# edytlab — Tools Reference

> Complete reference for all 28 audio-editing tools available to the AI agent.
> Tools are defined in `crates/tools/src/tool/` and dispatched by the agent loop.

---

## Overview

Tools are deterministic functions the AI agent calls to manipulate the audio session. Each tool:
- Receives validated JSON input
- Reads from and/or writes to the `SessionState` via the `Store`
- Appends a new DAG node on state changes (non-destructive)
- Returns a JSON result or an error string

The agent selects tools based on your natural language instructions. You generally do not call tools directly — but understanding what they do helps you write more precise prompts.

**Example prompt → tool chain:**
```
"Cut the first 3 seconds and normalize the track to -14 LUFS"
  → cut_range(track_id="t1", start_sec=0, end_sec=3)
  → normalize(track_id="t1", target_lufs=-14)
```

---

## File and Track Management

### `load`

Load an audio file into a new session or add it as a new track.

**Input schema:**
```json
{
  "path": "string — absolute path to the audio file (MP3, WAV, FLAC)",
  "track_name": "string? — optional display name for the track"
}
```

**Output:** `{ "track_id": "string", "node_id": "string", "duration_sec": number }`

**Supported formats:** MP3, WAV, FLAC (via symphonia). Sample rate and channel count are detected automatically and normalized to the session sample rate (default 44100 Hz).

**Example prompt:** *"Load /Users/alice/vocals.wav"*

---

### `add_track`

Add a new empty track to the current session.

**Input schema:**
```json
{
  "name": "string — display name for the new track"
}
```

**Output:** `{ "track_id": "string", "node_id": "string" }`

---

### `remove_track`

Remove a track from the session. Does not delete the source audio file.

**Input schema:**
```json
{
  "track_id": "string — ID of the track to remove"
}
```

**Output:** `{ "node_id": "string" }`

---

## Region Editing

### `cut_range`

Remove a time range from a track. Audio after the cut point shifts left.

**Input schema:**
```json
{
  "track_id": "string",
  "start_sec": "number — start of range to remove",
  "end_sec": "number — end of range to remove"
}
```

**Output:** `{ "node_id": "string" }`

**Notes:**
- `start_sec` must be < `end_sec`
- Both values must be within the track's duration
- The region is not added to the clipboard (use `copy_region` first if needed)

**Example prompt:** *"Remove the section between 1:20 and 1:45"*

---

### `copy_region`

Copy a time region from a track to the clipboard.

**Input schema:**
```json
{
  "track_id": "string",
  "start_sec": "number",
  "end_sec": "number"
}
```

**Output:** `{ "duration_sec": number }` — length of the copied region.

---

### `paste_region`

Insert the clipboard contents into a track at the specified position.

**Input schema:**
```json
{
  "track_id": "string",
  "insert_at_sec": "number — position to insert (audio shifts right)"
}
```

**Output:** `{ "node_id": "string" }`

**Throws:** If clipboard is empty.

---

### `trim`

Remove silence from the start and/or end of a track.

**Input schema:**
```json
{
  "track_id": "string",
  "threshold_db": "number? — silence threshold in dBFS (default: -60)",
  "trim_start": "boolean? — trim leading silence (default: true)",
  "trim_end": "boolean? — trim trailing silence (default: true)"
}
```

**Output:** `{ "node_id": "string", "trimmed_start_sec": number, "trimmed_end_sec": number }`

**Example prompt:** *"Remove the silence at the start of track 1"*

---

### `insert_silence`

Insert a gap of silence at a position in a track.

**Input schema:**
```json
{
  "track_id": "string",
  "position_sec": "number",
  "duration_sec": "number — length of silence to insert"
}
```

**Output:** `{ "node_id": "string" }`

---

### `reverse`

Reverse a region of a track (or the entire track).

**Input schema:**
```json
{
  "track_id": "string",
  "start_sec": "number? — defaults to 0",
  "end_sec": "number? — defaults to track end"
}
```

**Output:** `{ "node_id": "string" }`

---

## Volume and Dynamics

### `gain`

Apply a static dB gain adjustment to a region of a track.

**Input schema:**
```json
{
  "track_id": "string",
  "amount_db": "number — gain in dB (-60 to +12)",
  "start_sec": "number? — defaults to 0",
  "end_sec": "number? — defaults to track end"
}
```

**Output:** `{ "node_id": "string" }`

**Example prompt:** *"Boost the vocals by 3 dB"*

---

### `set_track_gain`

Set the overall gain level for a track (affects the entire track, not a region).

**Input schema:**
```json
{
  "track_id": "string",
  "gain_db": "number — new gain level (-60 to +12)"
}
```

**Output:** `{ "node_id": "string" }`

---

### `normalize`

Normalize a track to a loudness target.

**Input schema:**
```json
{
  "track_id": "string",
  "target_lufs": "number? — integrated LUFS target (e.g. -14 for Spotify, -16 for Apple Podcasts, -23 for broadcast)",
  "target_peak_db": "number? — true peak limit in dBTP (e.g. -1.0)"
}
```

At least one of `target_lufs` or `target_peak_db` is required.

**Output:** `{ "node_id": "string", "applied_gain_db": number }`

**Common targets:**

| Platform | Integrated LUFS | True Peak |
|----------|----------------|-----------|
| Spotify | -14 LUFS | -1 dBTP |
| Apple Podcasts | -16 LUFS | -1 dBTP |
| YouTube | -14 LUFS | -1 dBTP |
| EBU R128 (broadcast) | -23 LUFS | -1 dBTP |
| CD mastering | -9 to -14 LUFS | 0 dBFS |

**Example prompt:** *"Normalize to -14 LUFS for Spotify"*

---

### `fade`

Apply a fade-in or fade-out envelope.

**Input schema:**
```json
{
  "track_id": "string",
  "kind": "\"in\" | \"out\"",
  "duration_sec": "number — length of the fade",
  "curve": "\"linear\" | \"exponential\" | \"logarithmic\"? — default: logarithmic"
}
```

**Output:** `{ "node_id": "string" }`

**Example prompt:** *"Add a 2-second fade-in and a 3-second fade-out"*

---

## Time and Pitch

### `time_stretch`

Change the duration of a track without changing its pitch.

**Input schema:**
```json
{
  "track_id": "string",
  "target_duration_sec": "number? — desired duration",
  "factor": "number? — stretch factor (0.5 = half speed, 2.0 = double speed)"
}
```

Provide either `target_duration_sec` or `factor`, not both.

**Output:** `{ "node_id": "string", "new_duration_sec": number }`

**Implementation:** Rubber Band (Phase 2) or dasp interpolation (Phase 1).

---

### `pitch_shift`

Change the pitch without changing the duration.

**Input schema:**
```json
{
  "track_id": "string",
  "semitones": "number — semitones to shift (-12 to +12)"
}
```

**Output:** `{ "node_id": "string" }`

**Example prompt:** *"Shift the vocals up 2 semitones"*

---

## Analysis

### `analyze_track`

Analyze a track for BPM, key, loudness, and transients.

**Input schema:**
```json
{
  "track_id": "string"
}
```

**Output:**
```json
{
  "bpm": 128.0,
  "key": "Am",
  "loudness_lufs": -12.3,
  "peak_dbfs": -0.5,
  "transient_count": 4820
}
```

---

### `align_to_beat`

Align the start of a track to the nearest beat (requires BPM to be known).

**Input schema:**
```json
{
  "track_id": "string",
  "reference_track_id": "string? — track whose BPM to use (defaults to first analyzed track)"
}
```

**Output:** `{ "node_id": "string", "shift_sec": number }`

---

## ML Tools

### `separate_stems`

Run Demucs stem separation on a track. Produces 4 new tracks: vocals, drums, bass, other.

**Input schema:**
```json
{
  "track_id": "string",
  "model": "\"htdemucs\" | \"htdemucs_6s\"? — default: htdemucs"
}
```

**Output:**
```json
{
  "node_id": "string",
  "stems": {
    "vocals":  "track_id",
    "drums":   "track_id",
    "bass":    "track_id",
    "other":   "track_id",
    "guitar":  "track_id",  // htdemucs_6s only
    "piano":   "track_id"   // htdemucs_6s only
  }
}
```

**Notes:**
- Runs 100% on-device. Model files (~80 MB) downloaded on first use.
- Processing time: ~45 sec/min of audio on Apple M3 (CPU). ~18–25 sec/min with CUDA (RTX 3060).
- `htdemucs_6s` takes ~2× longer and requires more memory.

**Example prompt:** *"Separate the stems on track 1"*

---

### `transcribe`

Transcribe audio to text using Whisper. Stores word-level timestamps in the session.

**Input schema:**
```json
{
  "track_id": "string",
  "language": "string? — BCP 47 language code (auto-detected if omitted)"
}
```

**Output:**
```json
{
  "node_id": "string",
  "word_count": 1240,
  "duration_sec": 3600.0,
  "language": "en"
}
```

**Notes:**
- Uses Whisper large-v3 (~1.5 GB). Downloaded on first use.
- A 60-minute file: ~4–8 minutes on CPU, faster with CoreML/CUDA.
- Transcript stored in `SessionState.transcript` and injectable into agent context.

**Example prompt:** *"Transcribe track 1"*

---

## DAG Operations

### `fork_node`

Fork the current DAG node to create an independent branch.

**Input schema:**
```json
{
  "label": "string? — label for the new branch node"
}
```

**Output:** `{ "node_id": "string" }` — the new forked node becomes the head.

---

### `revert_to`

Move the session head to an earlier node. Does not delete any existing nodes.

**Input schema:**
```json
{
  "target_node_id": "string — ID of the node to revert to"
}
```

**Output:** `{ "node_id": "string" }` — the target node is now the head.

---

### `compare_nodes`

Generate a human-readable diff between two DAG nodes.

**Input schema:**
```json
{
  "node_a": "string",
  "node_b": "string"
}
```

**Output:**
```json
{
  "tracks_added":   ["track_id_1"],
  "tracks_removed": [],
  "tracks_changed": [
    { "track_id": "t1", "gain_db_delta": 3.0 }
  ]
}
```

---

### `apply_diff`

Apply a computed session diff from `compare_nodes` to the current session.

**Input schema:**
```json
{
  "diff": "object — diff returned by compare_nodes"
}
```

**Output:** `{ "node_id": "string" }`

---

### `name_node`

Set a human-readable label on the current head node.

**Input schema:**
```json
{
  "label": "string"
}
```

**Output:** `{ "node_id": "string" }`

---

## Annotations

### `label`

Add a named marker or region annotation to the session timeline.

**Input schema:**
```json
{
  "name": "string — display name for the marker",
  "time_sec": "number? — for point markers",
  "start_sec": "number? — for region markers",
  "end_sec": "number? — for region markers"
}
```

Provide either `time_sec` (point marker) or `start_sec` + `end_sec` (region).

**Output:** `{ "annotation_id": "string" }`

**Example prompt:** *"Mark the chorus at 1:05"*

---

## Rendering

### `render_final`

Render the full session to a WAV file at the specified path.

**Input schema:**
```json
{
  "out_path": "string — absolute output path (must end in .wav)",
  "bit_depth": "16 | 24 | 32? — default: 24"
}
```

**Output:**
```json
{
  "path": "string",
  "duration_sec": 240.5,
  "peak_dbfs": -0.3,
  "sample_rate": 44100,
  "channels": 2
}
```

---

### `render_preview`

Render a preview WAV to a temp file for in-app playback.

**Input schema:**
```json
{
  "start_sec": "number? — defaults to 0",
  "end_sec": "number? — defaults to session end"
}
```

**Output:** `{ "path": "string" }` — temp file path. Valid for the app session duration.

---

## Prompt Tips

**Be specific about tracks:**
> "Normalize track 1 to -14 LUFS" is better than "normalize the track" when you have multiple tracks.

**Reference time by minutes:seconds:**
> "Cut from 0:00 to 0:05" is unambiguous. "Remove the first 5 seconds" also works.

**Chain operations in one message:**
> "Separate stems on track 1, then normalize the vocals stem to -14 LUFS, and export the vocals to /Users/alice/vocals-clean.wav"

**Use markers for reference points:**
> After adding markers, you can say "cut everything before the chorus marker".

**Let the agent pick defaults:**
> "Normalize this" — the agent picks a sensible LUFS target based on context (podcast vs. music).

---

*Last updated: 2026-05-17. Reflects edytlab v0.1.0-dev.*
