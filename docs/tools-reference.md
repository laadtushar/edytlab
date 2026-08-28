# edytlab — Tools Reference

> Every one of the 93 tools the AI agent can call: parameters, types, and what each one does.

<!-- GENERATED FILE — do not edit by hand.
     Regenerate with:
       UPDATE_TOOLS_REFERENCE=1 cargo test -p tools --test tools_reference_doc
     The source of truth is the tool registry itself
     (`ToolDispatcher::default_dispatcher()`), so this file cannot
     disagree with what the agent can actually call. -->

## What a tool is

Tools are deterministic functions the agent calls to manipulate the audio session. Each one receives JSON validated against the schema below, reads and writes `SessionState` through the `Store`, and appends a new DAG node when it changes something — so every edit is non-destructive and reversible.

You do not call tools directly. The agent picks them from what you ask for; this page is for knowing what exists and what it takes.

Implementations live in `crates/tools/src/tool/`. A tool that is not registered in `crates/tools/src/dispatcher.rs` is not on this page and the agent cannot call it.

## Index

- [`add_effect`](#add_effect) — Add a non-destructive effect to a track's chain.
- [`add_track`](#add_track) — Append a new empty track to the current session.
- [`align_to_beat`](#align_to_beat) — Warp a track in time so the beats at source_beats land on beat_grid, without changing its pitch.
- [`analyze_track`](#analyze_track) — Analyse a music file and return BPM, key, beat grid, downbeats, sections, an RMS curve (one bin per ~100 ms), and EBU R128 integrated loudness in LUFS.
- [`apply_diff`](#apply_diff) — Apply one or more SessionDiff specs to a parent node, producing one new sibling node per spec.
- [`apply_recipe`](#apply_recipe) — Replay an exported edit chain.
- [`audition_effect`](#audition_effect) — Hear what an effect would sound like on a track without applying it.
- [`batch_apply`](#batch_apply) — Run an exported edit chain across every audio file in a folder.
- [`change_speed`](#change_speed) — Resample a track to change playback speed without pitch preservation.
- [`click_removal`](#click_removal) — Remove clicks and pops by detecting sample spikes (via median filter) and replacing them with interpolated values.
- [`compact_session`](#compact_session) — Prune old history and delete the audio only it referenced, to reclaim disk.
- [`compare_nodes`](#compare_nodes) — Compute the structural diff between two session nodes.
- [`compressor`](#compressor) — Apply dynamic compression to a track using an envelope follower.
- [`copy_region`](#copy_region) — Copy a time range from a track into the in-memory clipboard.
- [`create_bus`](#create_bus) — Create an effects bus.
- [`cut_range`](#cut_range) — Remove the half-open sample range [start_sample, end_sample) from a track and shift the remainder left.
- [`cut_words`](#cut_words) — Delete a span of transcribed words and the audio underneath it, closing the gap.
- [`de_esser`](#de_esser) — Reduce harsh sibilant 's' and 'sh' sounds.
- [`distortion`](#distortion) — Apply soft-clip distortion (tanh waveshaper) followed by a tone filter.
- [`duck_under_speech`](#duck_under_speech) — Drop a music track under the speech and bring it back in the gaps, keyed on the transcript rather than on level.
- [`duplicate_track`](#duplicate_track) — Create an exact copy of a track (same clips, gain, pan, effects).
- [`echo`](#echo) — Add a single echo (delay + decay).
- [`eq`](#eq) — Apply a parametric equalizer (chain of biquad peak filters) to a track.
- [`export_labels`](#export_labels) — Export session annotations as Audacity-format label text (start_sec TAB end_sec TAB name, one per line).
- [`export_multiple`](#export_multiple) — Export selected tracks as individual WAV files to a directory.
- [`export_recipe`](#export_recipe) — Export this session's edit chain — every tool and its parameters, in order — to a JSON file with no audio in it.
- [`fade`](#fade) — Apply a linear fade-in or fade-out to a time range of a track.
- [`fork_node`](#fork_node) — Branch the session DAG at the given node id (defaults to the current head when `from` is omitted).
- [`gain`](#gain) — Apply a constant gain in decibels to a track.
- [`generate_noise`](#generate_noise) — Generate a noise track (white, pink, or brown/Brownian noise) and add it as a new track.
- [`generate_tone`](#generate_tone) — Synthesize a tone (sine, square, sawtooth, or triangle wave) and add it as a new track.
- [`high_pass_filter`](#high_pass_filter) — Apply a Butterworth high-pass filter to a track, removing frequencies below cutoff_hz.
- [`import_labels`](#import_labels) — Import Audacity-format label text into the session as annotations.
- [`insert_silence`](#insert_silence) — Insert a region of silence at a time offset in a track.
- [`invert`](#invert) — Invert (negate) audio polarity on a track, optionally within a time range.
- [`label`](#label) — Place a named marker or region label in the session at the current head.
- [`leveler`](#leveler) — Apply dynamic leveling: normalise each short window to a target RMS level.
- [`limiter`](#limiter) — Brick-wall limiter: hard-clip any samples exceeding ceiling_db.
- [`load`](#load) — Decode an audio file and add it to the session as a new track.
- [`low_pass_filter`](#low_pass_filter) — Apply a Butterworth low-pass filter to a track, removing frequencies above cutoff_hz.
- [`mix_to_new_track`](#mix_to_new_track) — Offline-render the selected tracks together and add the result as a new mixed track.
- [`mono_to_stereo`](#mono_to_stereo) — Convert a mono track to stereo by duplicating the channel to both L and R.
- [`move_clip`](#move_clip) — Move one clip to a new start position within its track, leaving the other clips where they are.
- [`mute_track`](#mute_track) — Mute or unmute a track.
- [`name_node`](#name_node) — Set or replace the human-readable label on a session node.
- [`noise_gate`](#noise_gate) — Apply a noise gate: audio below threshold_db is silenced.
- [`noise_reduction`](#noise_reduction) — Apply spectral noise reduction (spectral subtraction, overlap-add) to a track.
- [`normalize`](#normalize) — Scan a track for its peak amplitude and set its gain so the peak equals target_dbfs.
- [`normalize_loudness`](#normalize_loudness) — Set a track's gain so its integrated loudness (EBU R128) reaches target_lufs.
- [`notch_filter`](#notch_filter) — Apply a notch (band-reject) filter to a track, attenuating frequencies near center_hz.
- [`paste_region`](#paste_region) — Paste the clipboard audio (set by copy_region) into a track at a given offset.
- [`phaser`](#phaser) — Apply a phaser effect using an all-pass filter chain with LFO sweep.
- [`pitch_shift`](#pitch_shift) — Shift a track's pitch in semitones without changing its duration.
- [`plot_spectrum`](#plot_spectrum) — Compute the frequency spectrum of a track region and show the user a chart of it.
- [`punch_in`](#punch_in) — Replace a region of a track with audio from a file, in place.
- [`remove_clip`](#remove_clip) — Remove one clip from a track, leaving the other clips where they are.
- [`remove_effect`](#remove_effect) — Remove one effect from a track's chain by index.
- [`remove_fillers`](#remove_fillers) — Find filler words in the session's transcript and, when asked, remove them and their audio in one undoable edit.
- [`remove_send`](#remove_send) — Stop routing a track to a bus.
- [`remove_track`](#remove_track) — Remove a track from the session.
- [`rename_track`](#rename_track) — Rename a track.
- [`render_final`](#render_final) — Render a session node to a final audio file at the user's chosen path.
- [`render_preview`](#render_preview) — Render a session node to a temporary WAV file and return its path.
- [`reorder_effects`](#reorder_effects) — Reorder a track's effect chain.
- [`repeat_selection`](#repeat_selection) — Duplicate the audio region [start_sec, end_sec) on a track N additional times, appending copies after the original buffer.
- [`resample_track`](#resample_track) — Resample a track to a different sample rate using linear interpolation.
- [`reverb`](#reverb) — Apply Freeverb algorithmic reverb.
- [`reverse`](#reverse) — Reverse the sample order of a track, optionally within a sub-range.
- [`revert_to`](#revert_to) — Append a new node whose state matches the target node's state, parented to the current head.
- [`select_region`](#select_region) — Resolve a description of a region into a concrete time range, using the session's transcript and tempo map.
- [`separate_stems`](#separate_stems) — Run Demucs stem separation on an audio file and return paths to the four output stems (vocals/drums/bass/other) as WAVs.
- [`set_clip_envelope`](#set_clip_envelope) — Replace the per-clip volume automation curve.
- [`set_effect_bypassed`](#set_effect_bypassed) — Bypass or re-enable one effect without removing it, so its settings survive an A/B.
- [`set_effect_params`](#set_effect_params) — Change an effect's parameters in place.
- [`set_pan`](#set_pan) — Set the stereo pan of a track.
- [`set_send`](#set_send) — Route a copy of a track to a bus at the given level.
- [`set_sync_lock`](#set_sync_lock) — Turn sync-lock on or off.
- [`set_track_gain`](#set_track_gain) — Set a track's gain in decibels (absolute, not additive).
- [`silence_finder`](#silence_finder) — Analyse a track and return the time ranges of silent regions.
- [`silence_region`](#silence_region) — Zero out audio samples between start_sec and end_sec on a track.
- [`solo_track`](#solo_track) — Solo or un-solo a track.
- [`split_by_speaker`](#split_by_speaker) — Split one track into a separate track per speaker, given speaker segments in seconds.
- [`split_clip`](#split_clip) — Split a clip into two at the specified time position.
- [`stereo_to_mono`](#stereo_to_mono) — Convert a stereo (or multi-channel) track to mono by averaging all channels.
- [`stereo_widener`](#stereo_widener) — Widen or narrow the stereo field using M/S processing.
- [`storage_report`](#storage_report) — Report what this session is costing on disk.
- [`time_shift`](#time_shift) — Move a track's clips forward or backward in time.
- [`time_stretch`](#time_stretch) — Stretch or compress a track in time without changing its pitch.
- [`transcribe`](#transcribe) — Transcribe an audio file using the local Whisper-base ONNX model.
- [`tremolo`](#tremolo) — Apply tremolo (LFO amplitude modulation).
- [`trim`](#trim) — Keep only the half-open sample range [start_sample, end_sample) of a track and discard the rest.
- [`truncate_silence`](#truncate_silence) — Find and remove silent regions in a track.
- [`vocal_reduction`](#vocal_reduction) — Reduce center-panned vocals using L-R channel subtraction (Karaoke effect).

---

## `add_effect`

Add a non-destructive effect to a track's chain. Unlike the destructive effect tools, the parameters stay editable afterwards — set_effect_params can change them without re-running anything, because the effect is applied at render rather than baked into a new file. Effects apply in chain order, after track gain and volume automation and before pan. Appends a new session node.

| Parameter | Type | Required | Notes |
|---|---|---|---|
| `kind` | string | yes | Effect kind, e.g. gain, limiter, low_pass_filter, high_pass_filter, notch_filter. |
| `params` | object | no | Effect parameters, e.g. { "cutoff_hz": 800 }. Defaults are used for anything omitted. |
| `position` | integer | no | Index in the chain. Appended to the end when omitted. |
| `track` | integer | yes |  |

## `add_track`

Append a new empty track to the current session. Returns the new session node id and the new track's index. The track contributes silence until a clip is loaded onto it.

| Parameter | Type | Required | Notes |
|---|---|---|---|
| `name` | string | no |  |

Unlisted parameters are rejected: the dispatcher validates against this schema before the tool runs.

## `align_to_beat`

Warp a track in time so the beats at source_beats land on beat_grid, without changing its pitch. Use it to fix drifting timing or to conform a performance to a click. Get source_beats from analyze_track. The two arrays must be the same length — a mismatch is an error rather than a partial warp. Each segment between consecutive beats is stretched by its own ratio in a single pass, so there is no seam at the beats. This rewrites the track's audio and appends a new session node.

| Parameter | Type | Required | Notes |
|---|---|---|---|
| `beat_grid` | array of number | yes | Where those beats should end up, in seconds. Must have the same number of entries as source_beats. |
| `source_beats` | array of number | yes | Where the beats are now, in seconds from the start of the track. Get these from analyze_track. |
| `track` | integer | yes |  |

Unlisted parameters are rejected: the dispatcher validates against this schema before the tool runs.

## `analyze_track`

Analyse a music file and return BPM, key, beat grid, downbeats, sections, an RMS curve (one bin per ~100 ms), and EBU R128 integrated loudness in LUFS. Pure-Rust analysis: no model weights or env vars required. The audio is downmixed to mono internally for the music-feature passes; LUFS is measured on the original interleaved signal.

| Parameter | Type | Required | Notes |
|---|---|---|---|
| `path` | string | yes |  |

Unlisted parameters are rejected: the dispatcher validates against this schema before the tool runs.

## `apply_diff`

Apply one or more SessionDiff specs to a parent node, producing one new sibling node per spec. All new nodes are written atomically: either every branch lands on disk or none do. Use to author N alternative takes in a single step.

| Parameter | Type | Required | Notes |
|---|---|---|---|
| `branches` | array of object | yes |  |
| `from_node` | string | yes | hex node id of parent |

Unlisted parameters are rejected: the dispatcher validates against this schema before the tool runs.

## `apply_recipe`

Replay an exported edit chain. Every step is checked before any of them runs: if a step cannot be replayed the whole recipe is refused, naming the step, rather than half-applying it. Pass `source` to run the chain against different audio instead of the file it was recorded from. Pass `dry_run` to see what would happen without touching the session.

| Parameter | Type | Required | Notes |
|---|---|---|---|
| `dry_run` | boolean | no | Report the steps and any blockers without running them |
| `path` | string | yes | Recipe JSON to read |
| `source` | string | no | Optional audio file to run the chain against instead of the recorded one |

Unlisted parameters are rejected: the dispatcher validates against this schema before the tool runs.

## `audition_effect`

Hear what an effect would sound like on a track without applying it. Renders a few seconds of the session with the effect added to that track's chain and returns a WAV to play. Appends no session node, so there is nothing to undo — call `add_effect` with the same arguments to keep it. The audition includes gain, pan, mute, solo, sends and the master chain, so it sounds like the result will.

| Parameter | Type | Required | Notes |
|---|---|---|---|
| `at` | integer | no | Position in the chain; defaults to the end, as add_effect does. |
| `end_sec` | number | no | End of the region to hear |
| `kind` | string | yes | Effect kind, e.g. gain, limiter, low_pass_filter, high_pass_filter, notch_filter. |
| `params` | object | no | Effect parameters, the same shape add_effect takes. |
| `start_sec` | number | no | Start of the region to hear |
| `track` | integer | yes | Zero-based track index |

Unlisted parameters are rejected: the dispatcher validates against this schema before the tool runs.

## `batch_apply`

Run an exported edit chain across every audio file in a folder. Each file becomes its own project with its own history — a batch is not one giant session. Every file is attempted even if an earlier one fails, and the report says what succeeded, what refused, and why. Optionally renders each result.

| Parameter | Type | Required | Notes |
|---|---|---|---|
| `input_dir` | string | yes | Folder of audio to process |
| `output_dir` | string | no | Where the per-file projects go. Defaults to input_dir. |
| `recipe_path` | string | yes | Recipe JSON from export_recipe |
| `render_format` | one of `wav`, `flac`, `mp3` | no | Also render each result to this format beside its project. |

Unlisted parameters are rejected: the dispatcher validates against this schema before the tool runs.

## `change_speed`

Resample a track to change playback speed without pitch preservation. factor > 1 speeds up (shorter duration), factor < 1 slows down (longer). Appends a new session node.

| Parameter | Type | Required | Notes |
|---|---|---|---|
| `factor` | number | yes | Speed multiplier, e.g. 2.0 = double speed |
| `track` | integer | yes |  |

## `click_removal`

Remove clicks and pops by detecting sample spikes (via median filter) and replacing them with interpolated values. threshold is the amplitude deviation that triggers detection. Appends a new session node.

| Parameter | Type | Required | Notes |
|---|---|---|---|
| `end_sec` | number | no |  |
| `start_sec` | number | no |  |
| `threshold` | number | no | Amplitude spike threshold (linear, 0..1 scale) |
| `track` | integer | yes |  |

## `compact_session`

Prune old history and delete the audio only it referenced, to reclaim disk. This removes undo steps permanently — the nodes are gone, not archived. Reports what it would remove and changes nothing unless `apply` is true. The head's most recent `keep_last` nodes are never pruned, so ordinary undo keeps working; what goes is the tail beyond that and any abandoned branches. For reclaiming space without losing history, the derived-audio cache is swept automatically instead.

| Parameter | Type | Required | Notes |
|---|---|---|---|
| `apply` | boolean | no | True to actually prune. Omit to see what would go without doing it. |
| `keep_last` | integer | no | How many recent nodes on the head's chain to keep. Default 20. |

Unlisted parameters are rejected: the dispatcher validates against this schema before the tool runs.

## `compare_nodes`

Compute the structural diff between two session nodes. Returns a `SessionDiff` JSON object with `added`, `removed`, and `modified` arrays of operations; each op identifies its target (track id, effect index, etc.) so callers can detect overlaps. Read-only.

| Parameter | Type | Required | Notes |
|---|---|---|---|
| `a` | string | yes | hex id of base node |
| `b` | string | yes | hex id of target node |

Unlisted parameters are rejected: the dispatcher validates against this schema before the tool runs.

## `compressor`

Apply dynamic compression to a track using an envelope follower. Reduces the gain of loud passages above a threshold by a given ratio. Supports configurable attack/release times and optional makeup gain. Appends a new session node.

| Parameter | Type | Required | Notes |
|---|---|---|---|
| `attack_ms` | number | yes |  |
| `makeup_db` | number | no |  |
| `ratio` | number | yes |  |
| `release_ms` | number | yes |  |
| `threshold_db` | number | yes |  |
| `track` | integer | yes |  |

Unlisted parameters are rejected: the dispatcher validates against this schema before the tool runs.

## `copy_region`

Copy a time range from a track into the in-memory clipboard. Does not modify the session graph. Use paste_region to insert it.

| Parameter | Type | Required | Notes |
|---|---|---|---|
| `range` | object | no |  |
| `track` | integer | yes | Zero-based track index |

Unlisted parameters are rejected: the dispatcher validates against this schema before the tool runs.

## `create_bus`

Create an effects bus. Tracks send a scaled copy of themselves to a bus with `set_send`, the bus processes that sum, and the result is added to the master mix. Use this for a shared reverb or delay: one instance fed by several tracks, rather than the same effect applied destructively to each. Returns the bus id. Appends a new session node.

| Parameter | Type | Required | Notes |
|---|---|---|---|
| `name` | string | yes | e.g. "Reverb" |

Unlisted parameters are rejected: the dispatcher validates against this schema before the tool runs.

## `cut_range`

Remove the half-open sample range [start_sample, end_sample) from a track and shift the remainder left. Appends a new session node parented to the current head.

| Parameter | Type | Required | Notes |
|---|---|---|---|
| `end_sample` | integer | yes |  |
| `start_sample` | integer | yes |  |
| `track` | integer | yes |  |

Unlisted parameters are rejected: the dispatcher validates against this schema before the tool runs.

## `cut_words`

Delete a span of transcribed words and the audio underneath it, closing the gap. Indices are into the session's transcript, as returned by `transcribe`. The remaining word timings are shifted so they still line up with the audio, and the whole thing is one undoable node. Requires a transcript.

| Parameter | Type | Required | Notes |
|---|---|---|---|
| `from_word` | integer | yes | First word to remove |
| `to_word` | integer | yes | One past the last word to remove |
| `track` | integer | no | Track the transcript describes; defaults to 0 |

Unlisted parameters are rejected: the dispatcher validates against this schema before the tool runs.

## `de_esser`

Reduce harsh sibilant 's' and 'sh' sounds. frequency_hz sets where sibilance detection begins (default 7000Hz); threshold_db is the compression trigger level. Appends a new session node.

| Parameter | Type | Required | Notes |
|---|---|---|---|
| `end_sec` | number | no |  |
| `frequency_hz` | number | no |  |
| `start_sec` | number | no |  |
| `threshold_db` | number | yes | Detection threshold in dBFS (e.g. -20) |
| `track` | integer | yes |  |

## `distortion`

Apply soft-clip distortion (tanh waveshaper) followed by a tone filter. drive > 1 increases gain before clipping; tone (0=dark, 1=bright) controls the output filter. Appends a new session node.

| Parameter | Type | Required | Notes |
|---|---|---|---|
| `drive` | number | no | Pre-gain multiplier (1=clean, 10=heavy) |
| `tone` | number | no | Tone brightness 0..1 |
| `track` | integer | yes |  |

## `duck_under_speech`

Drop a music track under the speech and bring it back in the gaps, keyed on the transcript rather than on level. More accurate than a sidechain compressor — a breath does not trigger it and a quiet line does not escape it — and it can duck slightly before a line starts, which a level trigger cannot. Writes an ordinary volume-automation curve on the music track, so the result is visible and editable rather than a black box.

| Parameter | Type | Required | Notes |
|---|---|---|---|
| `attack_ms` | number | no | Time to drop. Default 120 ms. |
| `duck_db` | number | no | How far to drop, in dB. Default -12. |
| `join_gap_sec` | number | no | Speech gaps shorter than this do not un-duck. Default 1 s. |
| `music_track` | integer | yes | Track to duck |
| `pre_roll_ms` | number | no | Start ducking this long before a line. Default 150 ms. |
| `release_ms` | number | no | Time to recover. Default 400 ms. |

Unlisted parameters are rejected: the dispatcher validates against this schema before the tool runs.

## `duplicate_track`

Create an exact copy of a track (same clips, gain, pan, effects). The duplicate is appended after all existing tracks. Appends a new session node.

| Parameter | Type | Required | Notes |
|---|---|---|---|
| `track` | integer | yes |  |

## `echo`

Add a single echo (delay + decay). delay_ms is the echo offset in milliseconds; decay (0..1) is the echo amplitude. Appends a new session node.

| Parameter | Type | Required | Notes |
|---|---|---|---|
| `decay` | number | no | Echo amplitude 0..1 |
| `delay_ms` | number | yes | Echo delay in milliseconds |
| `end_sec` | number | no |  |
| `start_sec` | number | no |  |
| `track` | integer | yes |  |

## `eq`

Apply a parametric equalizer (chain of biquad peak filters) to a track. Each band specifies a centre frequency, gain in dB, and optional Q factor (default 1.0). Appends a new session node.

| Parameter | Type | Required | Notes |
|---|---|---|---|
| `bands` | array of object | yes |  |
| `track` | integer | yes |  |

Unlisted parameters are rejected: the dispatcher validates against this schema before the tool runs.

## `export_labels`

Export session annotations as Audacity-format label text (start_sec TAB end_sec TAB name, one per line). Does not modify audio. Returns the label text.

Takes no parameters.

## `export_multiple`

Export selected tracks as individual WAV files to a directory. Does not modify the session. Returns list of exported file paths.

| Parameter | Type | Required | Notes |
|---|---|---|---|
| `format` | one of `wav` | no |  |
| `output_dir` | string | yes | Directory to write exported files into |
| `track_indices` | array of integer | yes | Indices of tracks to export |

## `export_recipe`

Export this session's edit chain — every tool and its parameters, in order — to a JSON file with no audio in it. The result can be reviewed by eye and replayed against the same source to reproduce it exactly, or against different audio. Steps that cannot be replayed (ML models) are marked, and the file says so before anyone runs it.

| Parameter | Type | Required | Notes |
|---|---|---|---|
| `name` | string | no | Optional human name for the chain |
| `out_path` | string | yes | Where to write the recipe JSON |

Unlisted parameters are rejected: the dispatcher validates against this schema before the tool runs.

## `fade`

Apply a linear fade-in or fade-out to a time range of a track. Requires a range (start_sec, end_sec). Kind defaults to 'out'. Appends a new session node.

| Parameter | Type | Required | Notes |
|---|---|---|---|
| `kind` | one of `in`, `out` | no |  |
| `range` | object | no |  |
| `track` | integer | yes |  |

Unlisted parameters are rejected: the dispatcher validates against this schema before the tool runs.

## `fork_node`

Branch the session DAG at the given node id (defaults to the current head when `from` is omitted). Sets head to that node so subsequent mutating tools form a new branch parented off it. Returns the branch-point node id.

| Parameter | Type | Required | Notes |
|---|---|---|---|
| `from` | string | no | hex node id; defaults to current head |

Unlisted parameters are rejected: the dispatcher validates against this schema before the tool runs.

## `gain`

Apply a constant gain in decibels to a track. Composes additively with any existing track gain. Appends a new session node parented to the current head.

| Parameter | Type | Required | Notes |
|---|---|---|---|
| `db` | number | yes |  |
| `track` | integer | yes |  |

Unlisted parameters are rejected: the dispatcher validates against this schema before the tool runs.

## `generate_noise`

Generate a noise track (white, pink, or brown/Brownian noise) and add it as a new track.

| Parameter | Type | Required | Notes |
|---|---|---|---|
| `amplitude` | number | no |  |
| `duration_sec` | number | yes |  |
| `noise_type` | one of `white`, `pink`, `brown` | no |  |

## `generate_tone`

Synthesize a tone (sine, square, sawtooth, or triangle wave) and add it as a new track. Returns the new track index.

| Parameter | Type | Required | Notes |
|---|---|---|---|
| `amplitude` | number | no | Peak amplitude 0..1 |
| `duration_sec` | number | yes | Duration in seconds |
| `frequency_hz` | number | yes | Tone frequency in Hz |
| `waveform` | one of `sine`, `square`, `sawtooth`, `triangle` | no |  |

## `high_pass_filter`

Apply a Butterworth high-pass filter to a track, removing frequencies below cutoff_hz. Appends a new session node.

| Parameter | Type | Required | Notes |
|---|---|---|---|
| `cutoff_hz` | number | yes | Cutoff frequency in Hz |
| `end_sec` | number | no |  |
| `start_sec` | number | no |  |
| `track` | integer | yes |  |

## `import_labels`

Import Audacity-format label text into the session as annotations. Format: each line is 'start_sec TAB end_sec TAB name'. Appends a new session node.

| Parameter | Type | Required | Notes |
|---|---|---|---|
| `labels_text` | string | yes | Label file content in Audacity format |

## `insert_silence`

Insert a region of silence at a time offset in a track. Extends the track length by `duration` seconds. Appends a new session node.

| Parameter | Type | Required | Notes |
|---|---|---|---|
| `at` | number | yes | Offset in seconds where silence is inserted |
| `duration` | number | yes | Duration of silence in seconds |
| `track` | integer | yes |  |

Unlisted parameters are rejected: the dispatcher validates against this schema before the tool runs.

## `invert`

Invert (negate) audio polarity on a track, optionally within a time range. Appends a new session node.

| Parameter | Type | Required | Notes |
|---|---|---|---|
| `end_sec` | number | no | End of invert region in seconds (exclusive). Omit to invert to end of track. |
| `start_sec` | number | no | Start of invert region in seconds (inclusive). Omit to start at 0. |
| `track` | integer | yes | Zero-based track index |

Unlisted parameters are rejected: the dispatcher validates against this schema before the tool runs.

## `label`

Place a named marker or region label in the session at the current head. Provide `time` for a point marker or `start`+`end` for a region. Appends a new session node.

| Parameter | Type | Required | Notes |
|---|---|---|---|
| `end` | number | no | Region end in seconds |
| `name` | string | yes | Display name for the label |
| `start` | number | no | Region start in seconds |
| `time` | number | no | Marker position in seconds (point marker) |

Unlisted parameters are rejected: the dispatcher validates against this schema before the tool runs.

## `leveler`

Apply dynamic leveling: normalise each short window to a target RMS level. Reduces variation between loud and quiet passages. Appends a new session node.

| Parameter | Type | Required | Notes |
|---|---|---|---|
| `end_sec` | number | no |  |
| `start_sec` | number | no |  |
| `target_db` | number | yes | Target RMS level in dBFS (e.g. -18) |
| `track` | integer | yes |  |

## `limiter`

Brick-wall limiter: hard-clip any samples exceeding ceiling_db. Prevents digital clipping. Appends a new session node.

| Parameter | Type | Required | Notes |
|---|---|---|---|
| `ceiling_db` | number | yes | Maximum peak level in dBFS (e.g. -1.0) |
| `end_sec` | number | no |  |
| `start_sec` | number | no |  |
| `track` | integer | yes |  |

## `load`

Decode an audio file and add it to the session as a new track. With no current head this creates a fresh single-track session; otherwise the file is appended as a new track on the current head, leaving existing tracks intact. Returns the new session node id, the new track's index, and the source's sample rate, length, and channel count.

| Parameter | Type | Required | Notes |
|---|---|---|---|
| `path` | string | yes |  |

Unlisted parameters are rejected: the dispatcher validates against this schema before the tool runs.

## `low_pass_filter`

Apply a Butterworth low-pass filter to a track, removing frequencies above cutoff_hz. Appends a new session node.

| Parameter | Type | Required | Notes |
|---|---|---|---|
| `cutoff_hz` | number | yes |  |
| `end_sec` | number | no |  |
| `start_sec` | number | no |  |
| `track` | integer | yes |  |

## `mix_to_new_track`

Offline-render the selected tracks together and add the result as a new mixed track. track_indices selects which tracks to include. Appends a new session node.

| Parameter | Type | Required | Notes |
|---|---|---|---|
| `name` | string | no | Name for the new track |
| `track_indices` | array of integer | yes | Indices of tracks to mix |

## `mono_to_stereo`

Convert a mono track to stereo by duplicating the channel to both L and R. Appends a new session node.

| Parameter | Type | Required | Notes |
|---|---|---|---|
| `track` | integer | yes |  |

## `move_clip`

Move one clip to a new start position within its track, leaving the other clips where they are. Use time_shift to move a whole track together. Appends a new session node.

| Parameter | Type | Required | Notes |
|---|---|---|---|
| `clip_index` | integer | yes |  |
| `start_sec` | number | yes | New start of the clip, in seconds from the top of the timeline. |
| `track` | integer | yes |  |

## `mute_track`

Mute or unmute a track. Muted tracks produce silence in the mix. Appends a new session node.

| Parameter | Type | Required | Notes |
|---|---|---|---|
| `muted` | boolean | yes |  |
| `track` | integer | yes |  |

## `name_node`

Set or replace the human-readable label on a session node. Metadata only; does NOT change the node id (which is content-hashed over state, not label).

| Parameter | Type | Required | Notes |
|---|---|---|---|
| `label` | string | yes | new label; pass an empty string to clear |
| `node_id` | string | yes | hex id of the node to label |

Unlisted parameters are rejected: the dispatcher validates against this schema before the tool runs.

## `noise_gate`

Apply a noise gate: audio below threshold_db is silenced. attack_ms and release_ms control how fast the gate opens/closes. Appends a new session node.

| Parameter | Type | Required | Notes |
|---|---|---|---|
| `attack_ms` | number | no | Gate open time in ms |
| `end_sec` | number | no |  |
| `release_ms` | number | no | Gate close time in ms |
| `start_sec` | number | no |  |
| `threshold_db` | number | yes | Gate threshold in dBFS (e.g. -40) |
| `track` | integer | yes |  |

## `noise_reduction`

Apply spectral noise reduction (spectral subtraction, overlap-add) to a track. The first `noise_duration_sec` seconds of the clip are used as the noise profile. `strength` controls how aggressively noise is subtracted (default 0.85); `floor` sets the minimum fraction of the original magnitude retained to avoid musical-noise artefacts (default 0.05). Appends a new session node.

| Parameter | Type | Required | Notes |
|---|---|---|---|
| `floor` | number | no |  |
| `noise_duration_sec` | number | yes |  |
| `strength` | number | no |  |
| `track` | integer | yes |  |

Unlisted parameters are rejected: the dispatcher validates against this schema before the tool runs.

## `normalize`

Scan a track for its peak amplitude and set its gain so the peak equals target_dbfs. The source file is not rewritten; the engine applies the gain at render time. Appends a new session node parented to the current head.

| Parameter | Type | Required | Notes |
|---|---|---|---|
| `target_dbfs` | number | yes |  |
| `track` | integer | yes |  |

Unlisted parameters are rejected: the dispatcher validates against this schema before the tool runs.

## `normalize_loudness`

Set a track's gain so its integrated loudness (EBU R128) reaches target_lufs. Use this rather than `normalize` for delivery: -14 LUFS for Spotify and YouTube, -16 for Apple Podcasts, -23 for broadcast. Peak normalisation cannot match perceived loudness between files. If the gain needed would push peaks above true_peak_ceiling_db (default -1 dBFS), it is capped there instead of clipping, and the result reports the shortfall in `achieved_lufs` and `shortfall_db` — run `limiter` first if you need to close it. The source file is not rewritten; the engine applies the gain at render time.

| Parameter | Type | Required | Notes |
|---|---|---|---|
| `preset` | one of `spotify`, `youtube`, `apple_podcasts`, `broadcast` | no | A delivery target by name instead of a number: spotify/youtube = -14 LUFS, apple_podcasts = -16, broadcast = -23. |
| `target_lufs` | number | no | e.g. -14 for streaming, -23 for broadcast. Omit when `preset` is given. |
| `track` | integer | yes |  |
| `true_peak_ceiling_db` | number | no | Peak ceiling in dBFS; gain is capped to respect it. Default -1.0 |

Unlisted parameters are rejected: the dispatcher validates against this schema before the tool runs.

## `notch_filter`

Apply a notch (band-reject) filter to a track, attenuating frequencies near center_hz. q controls the width: higher Q = narrower notch. Appends a new session node.

| Parameter | Type | Required | Notes |
|---|---|---|---|
| `center_hz` | number | yes | Center frequency to reject in Hz |
| `end_sec` | number | no |  |
| `q` | number | yes | Quality factor (sharpness); typical range 0.5..30 |
| `start_sec` | number | no |  |
| `track` | integer | yes |  |

## `paste_region`

Paste the clipboard audio (set by copy_region) into a track at a given offset. The clipboard audio is spliced in; samples after the insertion point are shifted right. Appends a new session node.

| Parameter | Type | Required | Notes |
|---|---|---|---|
| `at` | number | yes | Insertion point in seconds; past the end appends |
| `track` | integer | yes | Zero-based track index |

Unlisted parameters are rejected: the dispatcher validates against this schema before the tool runs.

## `phaser`

Apply a phaser effect using an all-pass filter chain with LFO sweep. rate_hz controls LFO speed; depth is the wet blend; stages sets the filter chain length (2-12). Appends a new session node.

| Parameter | Type | Required | Notes |
|---|---|---|---|
| `depth` | number | no |  |
| `rate_hz` | number | no |  |
| `stages` | integer | no |  |
| `track` | integer | yes |  |

## `pitch_shift`

Shift a track's pitch in semitones without changing its duration. +12 is one octave up, -12 one octave down; the range is +/-48. Set `preserve_formants` for voices: it holds the resonances of the vocal tract where they were while the harmonics move, so a shifted voice sounds like the same person singing higher rather than the classic chipmunk or giant. Leave it off for instruments and for material where the character should move with the pitch. It is most reliable within about a fifth; larger shifts are limited by the vocoder rather than by the correction. Quality is a phase vocoder's: sustained material is clean and attacks are preserved by onset-triggered phase resets, though dense material can sound slightly phasey. Appends a new session node.

| Parameter | Type | Required | Notes |
|---|---|---|---|
| `preserve_formants` | boolean | no |  |
| `semitones` | number | yes |  |
| `track` | integer | yes |  |

Unlisted parameters are rejected: the dispatcher validates against this schema before the tool runs.

## `plot_spectrum`

Compute the frequency spectrum of a track region and show the user a chart of it. Returns the analysis you need as numbers: peak frequency and level, energy per band (sub/bass/low_mid/mid/high_mid/air) in dBFS, spectral centroid (brightness), 85% rolloff, and the noise floor. Use these to decide on EQ moves. Does not modify audio.

| Parameter | Type | Required | Notes |
|---|---|---|---|
| `end_sec` | number | yes | Region end in seconds |
| `start_sec` | number | yes | Region start in seconds |
| `track` | integer | yes |  |

## `punch_in`

Replace a region of a track with audio from a file, in place. The region's length is unchanged, so everything after the punch stays where it was — this is how a misread line gets fixed without re-recording the take or shifting the rest of the session. A take longer than the region is trimmed; a shorter one is padded with silence, and the tool reports which happened. Appends one undoable node.

| Parameter | Type | Required | Notes |
|---|---|---|---|
| `end_sec` | number | yes | End of the punch region |
| `start_sec` | number | yes | Start of the punch region |
| `take_path` | string | yes | Audio file holding the retake, e.g. what stop_recording wrote. |
| `track` | integer | yes | Zero-based track index |

Unlisted parameters are rejected: the dispatcher validates against this schema before the tool runs.

## `remove_clip`

Remove one clip from a track, leaving the other clips where they are. The gap it leaves is silence — this does not close up the timeline. Use remove_track to drop a whole track. Appends a new session node.

| Parameter | Type | Required | Notes |
|---|---|---|---|
| `clip_index` | integer | yes |  |
| `track` | integer | yes |  |

## `remove_effect`

Remove one effect from a track's chain by index. The rest keep their order. Appends a new session node.

| Parameter | Type | Required | Notes |
|---|---|---|---|
| `effect_index` | integer | yes |  |
| `track` | integer | yes |  |

## `remove_fillers`

Find filler words in the session's transcript and, when asked, remove them and their audio in one undoable edit. Reports by default without changing anything: this is a destructive edit across a whole track, so it says what it found and waits. Removes hesitations (um, uh, er) wherever they appear, but discourse markers (like, actually) only where they stand alone between pauses — speech with every hesitation stripped sounds rushed. Leaves a short pause where each filler was so the result does not sound spliced.

| Parameter | Type | Required | Notes |
|---|---|---|---|
| `apply` | boolean | no | Remove them. Omit to report what would be removed without touching the session. |
| `keep_gap_ms` | number | no | Pause to leave where each filler was, in milliseconds. Default 80. |
| `track` | integer | no | Track the transcript describes; defaults to 0 |
| `words` | array of string | no | Replaces the built-in list. Fillers are language- and speaker-specific. |

Unlisted parameters are rejected: the dispatcher validates against this schema before the tool runs.

## `remove_send`

Stop routing a track to a bus. The track keeps going to the master mix. Appends a new session node.

| Parameter | Type | Required | Notes |
|---|---|---|---|
| `bus_id` | string | yes |  |
| `track` | integer | yes |  |

Unlisted parameters are rejected: the dispatcher validates against this schema before the tool runs.

## `remove_track`

Remove a track from the session. Tracks at higher indices shift down by one. Returns the new session node id and the new track count.

| Parameter | Type | Required | Notes |
|---|---|---|---|
| `track` | integer | yes |  |

Unlisted parameters are rejected: the dispatcher validates against this schema before the tool runs.

## `rename_track`

Rename a track. Appends a new session node.

| Parameter | Type | Required | Notes |
|---|---|---|---|
| `name` | string | yes |  |
| `track` | integer | yes |  |

## `render_final`

Render a session node to a final audio file at the user's chosen path. format="wav" is uncompressed, "flac" is lossless at roughly half the size and identical audio, "mp3" is lossy but plays anywhere — prefer flac when the user wants to send a file somewhere and quality matters, mp3 when size or compatibility matters. bitrate_kbps applies to mp3 only and defaults to 192.

| Parameter | Type | Required | Notes |
|---|---|---|---|
| `bitrate_kbps` | integer | no | MP3 CBR target; snapped to the nearest valid Layer III rate. Ignored for wav and flac. |
| `format` | one of `wav`, `flac`, `mp3` | yes |  |
| `markers_as_chapters` | boolean | no | Write the session's markers as chapters. Off by default — a marker is a working annotation, and not every one is a chapter worth shipping. |
| `metadata` | object | no | Tags for the exported file. FLAC gets Vorbis comments, MP3 gets ID3v2. WAV has no standard tag container worth using and ignores this. |
| `node_id` | string | yes |  |
| `out_path` | string | yes |  |

Unlisted parameters are rejected: the dispatcher validates against this schema before the tool runs.

## `render_preview`

Render a session node to a temporary WAV file and return its path. Does not create a new session node. Optional `range` is a [start_sample, end_sample) pair into the rendered output.

| Parameter | Type | Required | Notes |
|---|---|---|---|
| `node_id` | string | yes |  |
| `range` | array of integer | no |  |

Unlisted parameters are rejected: the dispatcher validates against this schema before the tool runs.

## `reorder_effects`

Reorder a track's effect chain. `order` is the new sequence given as indices into the current chain — [2, 0, 1] moves the third effect to the front. Order matters: a compressor before an EQ is a different sound from one after it. Appends a new session node.

| Parameter | Type | Required | Notes |
|---|---|---|---|
| `order` | array of integer | yes | A permutation of the current indices. Must list every effect exactly once. |
| `track` | integer | yes |  |

## `repeat_selection`

Duplicate the audio region [start_sec, end_sec) on a track N additional times, appending copies after the original buffer. Appends a new session node.

| Parameter | Type | Required | Notes |
|---|---|---|---|
| `end_sec` | number | yes | End of region to repeat in seconds (exclusive) |
| `start_sec` | number | yes | Start of region to repeat in seconds (inclusive) |
| `times` | integer | yes | Number of additional copies to append |
| `track` | integer | yes | Zero-based track index |

Unlisted parameters are rejected: the dispatcher validates against this schema before the tool runs.

## `resample_track`

Resample a track to a different sample rate using linear interpolation. Common rates: 22050, 44100, 48000, 96000. Appends a new session node.

| Parameter | Type | Required | Notes |
|---|---|---|---|
| `target_sample_rate` | integer | yes | Target sample rate in Hz (e.g. 44100, 48000) |
| `track` | integer | yes | Track index |

## `reverb`

Apply Freeverb algorithmic reverb. room_size (0-1) controls reverb length, damping (0-1) controls high-freq decay, wet (0-1) is the wet/dry blend. Appends a new session node.

| Parameter | Type | Required | Notes |
|---|---|---|---|
| `damping` | number | no | High-freq damping 0..1 |
| `end_sec` | number | no |  |
| `room_size` | number | no | Room size 0..1 |
| `start_sec` | number | no |  |
| `track` | integer | yes |  |
| `wet` | number | no | Wet mix 0..1 |

## `reverse`

Reverse the sample order of a track, optionally within a sub-range. If range is omitted, the entire track is reversed. Appends a new session node.

| Parameter | Type | Required | Notes |
|---|---|---|---|
| `range` | object | no |  |
| `track` | integer | yes |  |

Unlisted parameters are rejected: the dispatcher validates against this schema before the tool runs.

## `revert_to`

Append a new node whose state matches the target node's state, parented to the current head. Useful for an 'undo to checkpoint' UX without losing the intermediate history.

| Parameter | Type | Required | Notes |
|---|---|---|---|
| `target` | string | yes | hex id of the node whose state to revert to |

Unlisted parameters are rejected: the dispatcher validates against this schema before the tool runs.

## `select_region`

Resolve a description of a region into a concrete time range, using the session's transcript and tempo map. Give exactly one of: `text` (a phrase to find in the transcript), `speech_passage` (1-based, or negative from the end — -1 is the last thing said), or `from_beat`/`to_beat`. Returns start_sec and end_sec for any tool that takes a range, and reports what it matched so the choice can be checked before acting on it. Refuses rather than guessing when the description does not resolve.

| Parameter | Type | Required | Notes |
|---|---|---|---|
| `from_beat` | integer | no |  |
| `occurrence` | integer | no | Which occurrence of `text`. Default 1. |
| `pad_sec` | number | no | Seconds of headroom either side. Default 0. |
| `speech_passage` | integer | no | A stretch of continuous speech. 1 is the first, -1 the last. |
| `text` | string | no | Phrase to find in the transcript, case-insensitive. |
| `to_beat` | integer | no |  |

Unlisted parameters are rejected: the dispatcher validates against this schema before the tool runs.

## `separate_stems`

Run Demucs stem separation on an audio file and return paths to the four output stems (vocals/drums/bass/other) as WAVs. Cached by content hash of the input plus the model file, so a second call with the same input returns the same paths without re-running inference. Default model is htdemucs_ft (best quality, slowest); pass model="htdemucs" for the faster, slightly lower-quality variant. Requires DEMUCS_MODEL_PATH / DEMUCS_FT_MODEL_PATH env vars.

| Parameter | Type | Required | Notes |
|---|---|---|---|
| `model` | one of `htdemucs_ft`, `htdemucs` | no |  |
| `path` | string | yes |  |

Unlisted parameters are rejected: the dispatcher validates against this schema before the tool runs.

## `set_clip_envelope`

Replace the per-clip volume automation curve. Points are (time_sec, gain_db) pairs; the engine linearly interpolates between them at render time. An empty points array clears any existing envelope (unity gain).

| Parameter | Type | Required | Notes |
|---|---|---|---|
| `clip_index` | integer | yes | Zero-based index of the clip within the track. |
| `points` | array of object | yes | Automation points sorted by time. Each point has time_sec (seconds from clip start) and gain_db. |
| `track_index` | integer | yes | Zero-based index of the target track. |

Unlisted parameters are rejected: the dispatcher validates against this schema before the tool runs.

## `set_effect_bypassed`

Bypass or re-enable one effect without removing it, so its settings survive an A/B. A bypassed effect renders identically to one that is absent. Appends a new session node.

| Parameter | Type | Required | Notes |
|---|---|---|---|
| `bypassed` | boolean | yes |  |
| `effect_index` | integer | yes |  |
| `track` | integer | yes |  |

## `set_effect_params`

Change an effect's parameters in place. This is what a non-destructive chain buys: the audio is not re-processed and nothing is re-rendered until you ask, so tweaking a cutoff costs nothing and is undoable like any other edit. Keys are merged into the existing parameters by default; pass replace=true to swap the object wholesale. Appends a new session node.

| Parameter | Type | Required | Notes |
|---|---|---|---|
| `effect_index` | integer | yes |  |
| `params` | object | yes |  |
| `replace` | boolean | no | Replace all parameters instead of merging. Default false. |
| `track` | integer | yes |  |

## `set_pan`

Set the stereo pan of a track. -1.0 = full left, 0.0 = centre, 1.0 = full right. Appends a new session node.

| Parameter | Type | Required | Notes |
|---|---|---|---|
| `pan` | number | yes |  |
| `track` | integer | yes |  |

## `set_send`

Route a copy of a track to a bus at the given level. The track still reaches the master mix at full level — this adds a parallel copy, which is how a send differs from moving the track onto the bus. Setting the same track and bus again replaces the level; use a very low level or remove_send to undo. The tap is post-fader, so changing the track's gain changes what it sends. Appends a new session node.

| Parameter | Type | Required | Notes |
|---|---|---|---|
| `bus_id` | string | yes | id returned by create_bus |
| `level_db` | number | yes | Level of the copy in dB; 0 sends at full level, -12 is subtle |
| `track` | integer | yes |  |

Unlisted parameters are rejected: the dispatcher validates against this schema before the tool runs.

## `set_sync_lock`

Turn sync-lock on or off. With it on, an edit that shifts time on one track shifts every track, so a multitrack recording stays aligned — cutting a sentence from one speaker's track in an interview moves the other speaker's track by the same amount rather than desynchronising the conversation. Affects `cut_range` and `insert_silence`; each is still one undoable node covering every track it moved.

| Parameter | Type | Required | Notes |
|---|---|---|---|
| `enabled` | boolean | yes | True to keep tracks aligned through edits that shift time. |

Unlisted parameters are rejected: the dispatcher validates against this schema before the tool runs.

## `set_track_gain`

Set a track's gain in decibels (absolute, not additive). Replaces any prior gain on the track. Use `gain` for additive composition.

| Parameter | Type | Required | Notes |
|---|---|---|---|
| `db` | number | yes |  |
| `track` | integer | yes |  |

Unlisted parameters are rejected: the dispatcher validates against this schema before the tool runs.

## `silence_finder`

Analyse a track and return the time ranges of silent regions. Does not modify audio. Returns a list of {start_sec, end_sec} objects.

| Parameter | Type | Required | Notes |
|---|---|---|---|
| `min_silence_ms` | number | no |  |
| `threshold_db` | number | yes | Silence floor in dBFS |
| `track` | integer | yes |  |

## `silence_region`

Zero out audio samples between start_sec and end_sec on a track. Appends a new session node parented to the current head.

| Parameter | Type | Required | Notes |
|---|---|---|---|
| `end_sec` | number | yes | End of silence region in seconds (exclusive) |
| `start_sec` | number | yes | Start of silence region in seconds (inclusive) |
| `track` | integer | yes | Zero-based track index |

Unlisted parameters are rejected: the dispatcher validates against this schema before the tool runs.

## `solo_track`

Solo or un-solo a track. When any track is soloed, only soloed tracks play in the mix. Appends a new session node.

| Parameter | Type | Required | Notes |
|---|---|---|---|
| `solo` | boolean | yes |  |
| `track` | integer | yes |  |

## `split_by_speaker`

Split one track into a separate track per speaker, given speaker segments in seconds. Each speaker's track references the same audio, so combined playback is unchanged — what changes is that each voice can now be mixed, EQ'd and de-noised on its own. Audio no segment covers goes to an 'unassigned' track so nothing is lost, and overlapping segments are awarded to the first one listed and reported back. Replaces the source track in place. Appends a new session node.

| Parameter | Type | Required | Notes |
|---|---|---|---|
| `segments` | array of object | yes | Speaker turns. Overlaps are awarded to the earlier entry. |
| `track` | integer | yes | Index of the track to split |
| `unassigned_name` | string | no | Name for the track holding audio no segment covers (default 'unassigned') |

## `split_clip`

Split a clip into two at the specified time position. Both resulting clips reference the same source file with adjusted offsets. Appends a new session node.

| Parameter | Type | Required | Notes |
|---|---|---|---|
| `at_sec` | number | yes | Position to split at, in seconds from track start |
| `clip_index` | integer | yes | Zero-based clip index within the track |
| `track` | integer | yes |  |

## `stereo_to_mono`

Convert a stereo (or multi-channel) track to mono by averaging all channels. Appends a new session node.

| Parameter | Type | Required | Notes |
|---|---|---|---|
| `track` | integer | yes |  |

## `stereo_widener`

Widen or narrow the stereo field using M/S processing. width=0 collapses to mono, width=1 is original, width=2 doubles the stereo width. Requires stereo track. Appends a new session node.

| Parameter | Type | Required | Notes |
|---|---|---|---|
| `track` | integer | yes |  |
| `width` | number | no | Stereo width (0=mono, 1=original, 2=extra wide) |

## `storage_report`

Report what this session is costing on disk. Every destructive edit writes a new audio file and none are ever deleted, so a long session grows without bound. Splits the derived audio three ways: files the current head needs, files only older nodes need (what undo is holding onto), and files no node references at all, plus what the bounded preview cache is holding. Reads only — it deletes nothing.

Takes no parameters.

## `time_shift`

Move a track's clips forward or backward in time. Positive offset_sec moves later, negative moves earlier (clamped to 0). Appends a new session node.

| Parameter | Type | Required | Notes |
|---|---|---|---|
| `offset_sec` | number | yes | Seconds to shift (positive=later, negative=earlier) |
| `track` | integer | yes |  |

## `time_stretch`

Stretch or compress a track in time without changing its pitch. factor=0.5 is 2x slower (twice as long), factor=2.0 is 2x faster (half as long). `preserve_formants` does nothing here and is accepted only because the two tools share a shape: a time stretch moves no frequency, so there are no formants to hold in place. Use it on `pitch_shift`, which does move them. Quality is a phase vocoder's: sustained material is clean and attacks are preserved by onset-triggered phase resets, but dense material can sound slightly phasey and factors far from 1.0 make that worse. Use `change_speed` instead when the pitch should move with the speed. Appends a new session node.

| Parameter | Type | Required | Notes |
|---|---|---|---|
| `factor` | number | yes |  |
| `preserve_formants` | boolean | no |  |
| `track` | integer | yes |  |

Unlisted parameters are rejected: the dispatcher validates against this schema before the tool runs.

## `transcribe`

Transcribe an audio file using the local Whisper-base ONNX model. Resamples internally to 16 kHz mono. Appends a new session node whose state carries the transcript; returns the produced words. Requires WHISPER_MODEL_PATH to point at the .onnx file (use scripts/fetch-models.sh).

| Parameter | Type | Required | Notes |
|---|---|---|---|
| `path` | string | yes |  |

Unlisted parameters are rejected: the dispatcher validates against this schema before the tool runs.

## `tremolo`

Apply tremolo (LFO amplitude modulation). rate_hz controls oscillation speed; depth (0..1) controls modulation depth. Appends a new session node.

| Parameter | Type | Required | Notes |
|---|---|---|---|
| `depth` | number | no | Modulation depth 0..1 |
| `rate_hz` | number | no | LFO rate in Hz |
| `track` | integer | yes |  |

## `trim`

Keep only the half-open sample range [start_sample, end_sample) of a track and discard the rest. Appends a new session node parented to the current head.

| Parameter | Type | Required | Notes |
|---|---|---|---|
| `end_sample` | integer | yes |  |
| `start_sample` | integer | yes |  |
| `track` | integer | yes |  |

Unlisted parameters are rejected: the dispatcher validates against this schema before the tool runs.

## `truncate_silence`

Find and remove silent regions in a track. threshold_db is the silence floor; min_silence_ms is the minimum gap duration to remove. Appends a new session node.

| Parameter | Type | Required | Notes |
|---|---|---|---|
| `min_silence_ms` | number | no | Minimum silence duration to remove in ms |
| `threshold_db` | number | yes | Silence threshold in dBFS (e.g. -60) |
| `track` | integer | yes |  |

## `vocal_reduction`

Reduce center-panned vocals using L-R channel subtraction (Karaoke effect). Works on stereo tracks; results depend on how centrally the vocals are mixed. Appends a new session node.

| Parameter | Type | Required | Notes |
|---|---|---|---|
| `end_sec` | number | no |  |
| `start_sec` | number | no |  |
| `track` | integer | yes |  |

