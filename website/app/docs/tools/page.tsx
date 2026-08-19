import type { Metadata } from "next";
import { siteConfig } from "@/lib/site";
import { DocShell } from "@/components/docs/doc-shell";

export const metadata: Metadata = {
  title: "Audio Tools Reference",
  description:
    "All 85 audio-editing tools available to the edytlab AI agent — cut, normalize, stem separate, transcribe, render, and more.",
  alternates: { canonical: "/docs/tools" },
  openGraph: {
    title: "Audio Tools Reference — edytlab Docs",
    description: "Complete reference for all 85 agent-callable audio tools.",
    url: `${siteConfig.url}/docs/tools`,
  },
};

const groups = [
  {
    title: "File and Track Management",
    tools: [
      {
        name: "load",
        prompt: 'load /path/to/file.wav',
        what: "Decode an audio file (MP3, WAV, FLAC) and create a new track in the session.",
        output: "track_id, duration_sec",
      },
      {
        name: "add_track",
        prompt: 'add an empty track called "drums"',
        what: "Add a new empty track to the session.",
        output: "track_id",
      },
      {
        name: "remove_track",
        prompt: 'remove track 2',
        what: "Remove a track. Does not delete the source file on disk.",
        output: "node_id",
      },
      {
        name: "duplicate_track",
        prompt: "duplicate track 1",
        what: "Create an exact copy of a track (same clips, gain, pan, effects). The duplicate is appended after all existing tracks.",
        output: "node_id",
      },
      {
        name: "rename_track",
        prompt: "rename track 2 to \"guitar\"",
        what: "Rename a track.",
        output: "node_id",
      },
      {
        name: "mute_track",
        prompt: "mute track 3",
        what: "Mute or unmute a track. Muted tracks produce silence in the mix.",
        output: "node_id",
      },
      {
        name: "solo_track",
        prompt: "solo the vocals",
        what: "Solo or un-solo a track. When any track is soloed, only soloed tracks play in the mix.",
        output: "node_id",
      },
      {
        name: "set_pan",
        prompt: "pan track 2 hard left",
        what: "Set the stereo pan of a track. -1.0 = full left, 0.0 = centre, 1.0 = full right.",
        output: "node_id",
      },
      {
        name: "time_shift",
        prompt: "move track 2 two seconds later",
        what: "Move a track's clips forward or backward in time. Positive offset_sec moves later, negative moves earlier (clamped to 0).",
        output: "node_id",
      },
      {
        name: "mix_to_new_track",
        prompt: "mix tracks 1 and 2 into a new track",
        what: "Offline-render the selected tracks together and add the result as a new mixed track. track_indices selects which tracks to include.",
        output: "node_id",
      }
    ],
  },
  {
    title: "Region Editing",
    tools: [
      {
        name: "cut_range",
        prompt: 'cut from 1:30 to 2:00 on track 1',
        what: "Remove a time range. Audio after the cut point shifts left.",
        output: "node_id, synced_tracks",
        note: "With sync-lock on, the same span comes out of every track so a multitrack recording stays aligned.",
      },
      {
        name: "set_sync_lock",
        prompt: "keep the tracks in sync when you cut",
        what: "Turn sync-lock on or off. With it on, an edit that shifts time on one track shifts every track. An interview is one track per speaker, and cutting a sentence from one of them leaves every later word on that track early while the other speaker stays put — the conversation comes apart, and nothing about the edit says it will. Affects cut_range and insert_silence.",
        output: "node_id, sync_lock, changed",
        note: "Each affected edit is still one undoable node covering every track it moved, so undo restores the whole edit rather than half of it. The mode is part of the session, so it survives Save As and the agent can read it before deciding whether a cut is safe.",
      },
      {
        name: "punch_in",
        prompt: "replace 1:20 to 1:35 with the retake I just recorded",
        what: "Replace a region of a track with audio from a file, in place. The region's length does not change, so everything after the punch stays exactly where it was — this is how a misread line gets fixed without re-recording the whole take or shifting the rest of the session out of sync.",
        output: "node_id, region_sec, take_sec, trimmed_sec, padded_sec",
        note: "A take longer than the region is trimmed and a shorter one is padded with silence, and the tool says which happened. It never stretches the performance and never ripples the timeline — rippling is exactly what punching in exists to avoid.",
      },
      {
        name: "copy_region",
        prompt: 'copy the section from 0:30 to 1:00',
        what: "Copy a time region to the clipboard, and persist it as a content-addressed blob so a later paste can be replayed rather than merely kept.",
        output: "clipboard_blob hash",
      },
      {
        name: "paste_region",
        prompt: 'paste at 2:00 on track 1',
        what: "Insert clipboard contents into a track. Audio shifts right at the insert point.",
        output: "node_id",
      },
      {
        name: "trim",
        prompt: 'remove the silence at the start of track 1',
        what: "Remove silence from the start and/or end of a track.",
        output: "node_id, trimmed_start_sec, trimmed_end_sec",
      },
      {
        name: "insert_silence",
        prompt: 'add 2 seconds of silence at 0:30',
        what: "Insert a gap of silence at a position. Audio shifts right.",
        output: "node_id",
      },
      {
        name: "reverse",
        prompt: 'reverse track 1',
        what: "Reverse a region (or the full track).",
        output: "node_id",
      },
      {
        name: "silence_region",
        prompt: "silence 0:10 to 0:12",
        what: "Zero out audio samples between start_sec and end_sec on a track.",
        output: "node_id",
      },
      {
        name: "repeat_selection",
        prompt: "repeat 0:00-0:04 three more times",
        what: "Duplicate the audio region [start_sec, end_sec) on a track N additional times, \\ appending copies after the original buffer.",
        output: "node_id",
      },
      {
        name: "split_clip",
        prompt: "split track 1 at 1:30",
        what: "Split a clip into two at the specified time position. Both resulting clips reference the same source file with adjusted offsets.",
        output: "node_id",
      },
      {
        name: "move_clip",
        prompt: "move the second clip on track 1 to 8 seconds",
        what: "Move one clip to a new start position, leaving the other clips where they are. Use time_shift to move a whole track together. Clips are re-sorted by start, so a clip dragged past its neighbour keeps the arrangement and the waveform in step.",
        output: "node_id",
      },
      {
        name: "remove_clip",
        prompt: "delete the second clip on track 1",
        what: "Remove one clip from a track. The gap it leaves is silence — this does not close up the timeline. Use remove_track to drop a whole track.",
        output: "node_id",
      },
      {
        name: "invert",
        prompt: "invert the polarity of track 2",
        what: "Invert (negate) audio polarity on a track, optionally within a time range.",
        output: "node_id",
      }
    ],
  },
  {
    title: "Volume and Dynamics",
    tools: [
      {
        name: "gain",
        prompt: 'boost the vocals by 3 dB',
        what: "Apply a static dB gain to a region of a track. Range: −60 to +12 dB.",
        output: "node_id",
      },
      {
        name: "set_track_gain",
        prompt: 'set track 2 gain to -3 dB',
        what: "Set the overall gain level for an entire track.",
        output: "node_id",
      },
      {
        name: "create_bus",
        prompt: 'make a reverb bus',
        what: "Create a mix bus with its own effect chain. Tracks feed it through sends, so one reverb can serve several tracks instead of being applied to each.",
        output: "node_id, bus_id",
      },
      {
        name: "set_send",
        prompt: 'send 30% of the vocal to the reverb bus',
        what: "Route a track to a bus at a given level in dB. The send is tapped after the track's own gain and pan.",
        output: "node_id",
      },
      {
        name: "remove_send",
        prompt: 'stop sending the vocal to the reverb bus',
        what: "Remove a track's send to a bus, leaving both the track and the bus in place.",
        output: "node_id",
      },
      {
        name: "normalize",
        prompt: 'normalize the peak to -1 dBFS',
        what: "Scan a track for its peak amplitude and set its gain so the peak lands on target_dbfs. Peak-based — two files normalised to the same peak can still differ by 10 LUFS in perceived loudness, so use normalize_loudness when the goal is 'as loud as everything else'.",
        output: "node_id, applied_gain_db",
      },
      {
        name: "normalize_loudness",
        prompt: 'normalize for Spotify',
        what: "Set gain so the track hits an integrated LUFS target, measured with EBU R128. Takes either a number or a platform by name — spotify, youtube, apple_podcasts, broadcast — so the target does not have to be remembered. Gain is capped at a true-peak ceiling (−1 dBFS by default) so it never clips getting there; when the cap bites, the result reports the shortfall rather than claiming success.",
        output: "node_id, measured_lufs, preset, applied_gain_db, achieved_lufs, shortfall_db, capped_by_ceiling",
        note: "Presets: spotify / youtube −14 LUFS, apple_podcasts −16, broadcast −23. A custom target_lufs still works.",
      },
      {
        name: "fade",
        prompt: 'add a 3-second fade-out',
        what: "Apply a fade-in or fade-out envelope. Curve options: linear, exponential, logarithmic.",
        output: "node_id",
      },
      {
        name: "set_clip_envelope",
        prompt: 'set a volume fade: track 0 clip 0, from 0s at -20dB to 2s at 0dB',
        what: "Set a per-clip volume automation curve. Provide (time_sec, gain_db) pairs and the engine linearly interpolates between them.",
        output: "node_id",
      },
      {
        name: "duck_under_speech",
        prompt: "duck the music under the voiceover",
        what: "Drop a music track under the speech and bring it back in the gaps, keyed on the transcript rather than on level. A sidechain compressor keys on level, so a breath triggers it and a quiet line escapes it; the transcript says where the words actually are. It also ducks slightly before each line starts, which a level trigger cannot do — it only knows a line began after it has.",
        output: "node_id, passages, ducks",
        note: "The result is an ordinary volume-automation curve on the music clip, so it is visible in the automation lane and draggable if a duck lands wrong. Short pauses inside a sentence do not un-duck: bringing the music up for a comma is a pump, not an edit.",
      },
      {
        name: "limiter",
        prompt: "limit track 1 to -1 dBFS",
        what: "Brick-wall limiter: hard-clip any samples exceeding ceiling_db. Prevents digital clipping.",
        output: "node_id",
      },
      {
        name: "noise_gate",
        prompt: "gate anything below -45 dB",
        what: "Apply a noise gate: audio below threshold_db is silenced. attack_ms and release_ms control how fast the gate opens/closes.",
        output: "node_id",
      },
      {
        name: "leveler",
        prompt: "even out the levels on track 1",
        what: "Apply dynamic leveling: normalise each short window to a target RMS level. Reduces variation between loud and quiet passages.",
        output: "node_id",
      },
      {
        name: "de_esser",
        prompt: "take the harshness off the s sounds",
        what: "Reduce harsh sibilant 's' and 'sh' sounds. frequency_hz sets where sibilance detection begins (default 7000Hz); threshold_db is the compression trigger level.",
        output: "node_id",
      },
      {
        name: "truncate_silence",
        prompt: "remove the long pauses",
        what: "Find and remove silent regions in a track. threshold_db is the silence floor; min_silence_ms is the minimum gap duration to remove.",
        output: "node_id",
      }
    ],
  },
  {
    title: "Effects",
    tools: [
      {
        name: "eq",
        prompt: 'boost the highs on track 1 by 3 dB at 8 kHz',
        what: "Apply a parametric EQ to a track using a chain of biquad peak filters. Specify frequency, gain (dB), and Q for each band.",
        output: "node_id",
      },
      {
        name: "compressor",
        prompt: 'compress track 1: threshold -18 dB, ratio 4:1',
        what: "Apply a dynamic compressor with configurable threshold, ratio, attack, and release. Uses an envelope follower for smooth gain reduction.",
        output: "node_id",
      },
      {
        name: "noise_reduction",
        prompt: 'reduce background noise on track 1',
        what: "Remove broadband noise via spectral subtraction (realFFT + overlap-add). Estimates the noise floor from a silent region and subtracts it from the signal.",
        output: "node_id",
      },
      {
        name: "reverb",
        prompt: "add a small room reverb",
        what: "Apply Freeverb algorithmic reverb. room_size (0-1) controls reverb length, damping (0-1) controls high-freq decay, wet (0-1) is the wet/dry blend.",
        output: "node_id",
      },
      {
        name: "echo",
        prompt: "add a 300 ms echo",
        what: "Add a single echo (delay + decay). delay_ms is the echo offset in milliseconds; decay (0..1) is the echo amplitude.",
        output: "node_id",
      },
      {
        name: "phaser",
        prompt: "add a slow phaser",
        what: "Apply a phaser effect using an all-pass filter chain with LFO sweep. rate_hz controls LFO speed; depth is the wet blend; stages sets the filter chain length (2-12).",
        output: "node_id",
      },
      {
        name: "tremolo",
        prompt: "add tremolo at 5 Hz",
        what: "Apply tremolo (LFO amplitude modulation). rate_hz controls oscillation speed; depth (0..1) controls modulation depth.",
        output: "node_id",
      },
      {
        name: "distortion",
        prompt: "drive track 2 a little",
        what: "Apply soft-clip distortion (tanh waveshaper) followed by a tone filter. drive > 1 increases gain before clipping; tone (0=dark, 1=bright) controls the output filter.",
        output: "node_id",
      },
      {
        name: "stereo_widener",
        prompt: "widen the stereo image",
        what: "Widen or narrow the stereo field using M/S processing. width=0 collapses to mono, width=1 is original, width=2 doubles the stereo width. Requires stereo track.",
        output: "node_id",
      },
      {
        name: "vocal_reduction",
        prompt: "take the vocals out",
        what: "Reduce center-panned vocals using L-R channel subtraction (Karaoke effect). Works on stereo tracks; results depend on how centrally the vocals are mixed.",
        output: "node_id",
      },
      {
        name: "click_removal",
        prompt: "remove the clicks",
        what: "Remove clicks and pops by detecting sample spikes (via median filter) and replacing them with interpolated values. threshold is the amplitude deviation that triggers detection.",
        output: "node_id",
      },
      {
        name: "low_pass_filter",
        prompt: "roll off everything above 8 kHz",
        what: "Apply a Butterworth low-pass filter to a track, removing frequencies above cutoff_hz.",
        output: "node_id",
      },
      {
        name: "high_pass_filter",
        prompt: "high-pass at 80 Hz",
        what: "Apply a Butterworth high-pass filter to a track, removing frequencies below cutoff_hz.",
        output: "node_id",
      },
      {
        name: "notch_filter",
        prompt: "notch out the 50 Hz hum",
        what: "Apply a notch (band-reject) filter to a track, attenuating frequencies near center_hz. q controls the width: higher Q = narrower notch.",
        output: "node_id",
      }
    ],
  },
  {
    title: "Effect Chains",
    tools: [
      {
        name: "add_effect",
        prompt: 'put a low-pass at 4 kHz on the guitar track',
        what: "Append an effect to a track's chain. The chain is applied at render time and the source audio is never rewritten, so the parameters stay editable — unlike the destructive effect tools above, which bake their result into a new node.",
        output: "node_id, effect_index",
        note: "Effects run in the order they appear in the chain. Not every effect can stream yet; ones that cannot are rejected at render time with a message naming them.",
      },
      {
        name: "set_effect_params",
        prompt: 'change that low-pass to 2 kHz',
        what: "Edit an effect already in a chain. Given parameters are merged into the existing ones by default; pass replace to swap the whole set instead.",
        output: "node_id",
      },
      {
        name: "set_effect_bypassed",
        prompt: 'bypass the compressor on track 1',
        what: "Turn an effect off without removing it. A bypassed effect renders byte-identically to one that is not there, so it is an A/B switch rather than an approximation.",
        output: "node_id",
      },
      {
        name: "reorder_effects",
        prompt: 'put the EQ before the compressor',
        what: "Reorder a track's chain. Takes a full permutation of the existing indices — a partial list is rejected rather than silently dropping effects.",
        output: "node_id",
      },
      {
        name: "remove_effect",
        prompt: 'take the reverb off the drums',
        what: "Remove an effect from a track's chain by index.",
        output: "node_id",
      }
    ],
  },
  {
    title: "Time and Pitch",
    tools: [
      {
        name: "time_stretch",
        prompt: 'stretch track 1 to 4 minutes',
        what: "Change the duration without changing the pitch.",
        output: "node_id, new_duration_sec",
      },
      {
        name: "pitch_shift",
        prompt: 'shift the vocals up 2 semitones',
        what: "Change the pitch without changing the duration. Set preserve_formants on voices so a shift sounds like the same person singing higher, not a chipmunk. Range: −12 to +12 semitones.",
        output: "node_id",
      },
      {
        name: "change_speed",
        prompt: "speed track 1 up by 10%",
        what: "Resample a track to change playback speed without pitch preservation. factor > 1 speeds up (shorter duration), factor < 1 slows down (longer).",
        output: "node_id",
      },
      {
        name: "resample_track",
        prompt: "resample track 1 to 44.1 kHz",
        what: "Resample a track to a different sample rate using linear interpolation. Common rates: 22050, 44100, 48000, 96000.",
        output: "node_id",
      }
    ],
  },
  {
    title: "Analysis",
    tools: [
      {
        name: "analyze_track",
        prompt: 'analyze track 1',
        what: "Detect BPM, musical key, integrated loudness (LUFS), true peak, and transient count.",
        output: "bpm, key, loudness_lufs, peak_dbfs, transient_count",
      },
      {
        name: "align_to_beat",
        prompt: 'find the beats, then warp this onto a steady grid',
        what: "Warp a track in time so its beats land on a target grid, without changing the pitch. Each segment between beats is stretched by its own ratio in one pass, so there is no seam at the beats. Get source_beats from analyze_track.",
        output: "node_id",
      },
      {
        name: "plot_spectrum",
        prompt: "show me the spectrum of track 1",
        what: "Compute the FFT magnitude spectrum of a track region.",
        output: "frequency/magnitude data",
      },
      {
        name: "silence_finder",
        prompt: "where are the silent bits?",
        what: "Analyse a track and return the time ranges of silent regions.",
        output: "list of {start_sec, end_sec}",
      },
      {
        name: "export_recipe",
        prompt: "export this edit chain so I can reuse it",
        what: "Write the session's edit chain — every tool and its parameters, in order — to a JSON file with no audio in it. Reviewable by eye before anyone runs it, and replayable against the same source or a different one.",
        output: "path, steps, blockers",
        note: "Steps that cannot be replayed (ML models) are marked in the file rather than silently dropped.",
      },
      {
        name: "apply_recipe",
        prompt: "run my podcast chain on this recording",
        what: "Replay an exported edit chain. Every step is checked before any of them runs, so a recipe that cannot keep its promise is refused whole — naming the step — rather than half-applied. Takes an optional different source, and a dry run that reports the plan without touching the session.",
        output: "steps_applied, head, notes",
        note: "Replaying against the same audio reproduces the same bytes; the derived files are named by their own samples, so a drifted rebuild is detected rather than substituted.",
      },
      {
        name: "audition_effect",
        prompt: "what would a 1 kHz low-pass sound like on the vocal?",
        what: "Hear an effect on a track without applying it. Renders a few seconds of the session with the effect added to that track's chain and hands back a WAV to play — no session node, so there is nothing to undo. Call add_effect with the same arguments to keep it.",
        output: "path, cached, start_sec, end_sec",
        note: "The audition includes gain, pan, mute, solo, sends and the master chain, so it sounds like the result will. Repeating settings you have already heard is instant.",
      },
      {
        name: "batch_apply",
        prompt: "run my podcast chain over every file in this folder",
        what: "Run an exported edit chain across every audio file in a folder. Each file becomes its own project with its own history — a batch is not one giant session. Every file is attempted even if an earlier one fails, and the report says what succeeded, what refused, and why.",
        output: "files, succeeded, refused, per-file results",
        note: "A chain that cannot be replayed is refused once, up front — that is a property of the chain, not of the twelve files.",
      },
      {
        name: "cut_words",
        prompt: "delete the bit where he repeats himself",
        what: "Delete a span of transcribed words and the audio underneath, closing the gap. Indices are into the session transcript from `transcribe`. The remaining word timings shift so they still line up with the audio, and the whole thing is one undoable node.",
        output: "node_id, removed_words, removed_text, removed_sec",
        note: "The span runs from the first word's start to the last word's end — cutting from the first word's end would leave a clipped syllable behind.",
      },
      {
        name: "remove_fillers",
        prompt: "how many ums are in this?",
        what: "Find filler words in the transcript and, when asked, remove them and their audio in one undoable edit. Reports by default without changing anything — this is a destructive edit across a whole track. Hesitations (um, uh, er) go wherever they appear; discourse markers (like, actually) only where they stand alone between pauses, because speech with every hesitation stripped sounds rushed.",
        output: "found, would_save_sec, per-word list; node_id when applied",
        note: "Leaves a short pause where each filler was, so the result does not sound spliced. The word list can be replaced — fillers are language- and speaker-specific.",
      },
      {
        name: "compact_session",
        prompt: "this project is huge — reclaim some disk",
        what: "Prune old history and delete the audio only it referenced. Reports what it would remove and changes nothing unless asked twice, because this removes undo steps permanently — the nodes are gone, not archived. The most recent nodes on the current chain are never pruned, so ordinary undo keeps working; what goes is the tail beyond them and any branches forked away from and never returned to.",
        output: "prunable_nodes, reclaimable_bytes; removed_nodes and freed_bytes when applied",
        note: "For space without losing history, the derived-audio cache sweeps itself: a file whose whole chain records a reproducible op regenerates byte-identically, so removing it costs a re-render on undo and nothing else.",
      },
      {
        name: "storage_report",
        prompt: "how much disk is this session using?",
        what: "Report what the session costs on disk, split by category: audio the current version needs, audio only the undo history needs (and how much of that is rebuildable from recorded operations), audio nothing references at all, the bounded preview cache, and clipboard blobs. Every destructive edit writes a new file and none are deleted, so a long session grows without bound. Reads only — it deletes nothing.",
        output: "total_bytes, live, history, unreferenced, preview_cache, clipboard_blobs",
        note: "There is no reclamation policy yet, so this measures the problem rather than solving it.",
      }
    ],
  },
  {
    title: "ML Tools",
    tools: [
      {
        name: "separate_stems",
        prompt: 'separate the stems on track 1',
        what: "Run Demucs stem separation on-device. Produces 4 tracks: vocals, drums, bass, other. Model: htdemucs (~80 MB). Processing: ~45 sec/min audio on CPU.",
        output: "node_id, stem track IDs",
        note: "First use downloads the model automatically. htdemucs_6s adds guitar and piano stems at ~2× the processing time.",
      },
      {
        name: "transcribe",
        prompt: 'transcribe track 1',
        what: "Transcribe spoken audio using Whisper large-v3 on-device. Stores word-level timestamps in the session. Model: ~1.5 GB. Processing: ~4–8 min per 60 min on CPU.",
        output: "node_id, word_count, language",
        note: "First use downloads the model automatically. CoreML (macOS) and CUDA significantly reduce processing time.",
      },
    ],
  },
  {
    title: "Generators",
    tools: [
      {
        name: "generate_tone",
        prompt: "generate a 440 Hz sine for 5 seconds",
        what: "Synthesize a tone (sine, square, sawtooth, or triangle wave) and add it as a new track.",
        output: "track index",
      },
      {
        name: "generate_noise",
        prompt: "generate 3 seconds of pink noise",
        what: "Generate a noise track (white, pink, or brown/Brownian noise) and add it as a new track.",
        output: "track index",
      },
    ],
  },
  {
    title: "Channel Layout",
    tools: [
      {
        name: "stereo_to_mono",
        prompt: "make track 1 mono",
        what: "Convert a stereo (or multi-channel) track to mono by averaging all channels.",
        output: "node_id",
      },
      {
        name: "mono_to_stereo",
        prompt: "make track 1 stereo",
        what: "Convert a mono track to stereo by duplicating the channel to both L and R.",
        output: "node_id",
      },
    ],
  },
  {
    title: "DAG Operations",
    tools: [
      {
        name: "fork_node",
        prompt: 'fork the session and call it "take-2"',
        what: "Fork the current node to create an independent branch. The fork becomes the new head.",
        output: "node_id",
      },
      {
        name: "revert_to",
        prompt: 'revert to before the reverb',
        what: "Move the session head to an earlier node. Does not delete any nodes.",
        output: "node_id",
      },
      {
        name: "compare_nodes",
        prompt: 'compare the current version with the one before normalization',
        what: "Generate a diff between two nodes: tracks added/removed, gain changes.",
        output: "tracks_added, tracks_removed, tracks_changed",
      },
      {
        name: "apply_diff",
        prompt: "(used internally by the agent)",
        what: "Apply a computed diff from compare_nodes to the current session.",
        output: "node_id",
      },
      {
        name: "name_node",
        prompt: 'name this state "final mix"',
        what: "Set a human-readable label on the current head node.",
        output: "node_id",
      },
    ],
  },
  {
    title: "Annotations",
    tools: [
      {
        name: "label",
        prompt: 'mark the chorus at 1:05',
        what: "Add a named point marker or region annotation to the timeline.",
        output: "annotation_id",
      },
      {
        name: "import_labels",
        prompt: "import these Audacity labels",
        what: "Import Audacity-format label text into the session as annotations. Format: each line is 'start_sec TAB end_sec TAB name'.",
        output: "node_id",
      },
      {
        name: "export_labels",
        prompt: "export the markers as labels",
        what: "Export session annotations as Audacity-format label text (start_sec TAB end_sec TAB name, one per line).",
        output: "label text",
      }
    ],
  },
  {
    title: "Rendering",
    tools: [
      {
        name: "render_final",
        prompt: 'export to /Users/me/Desktop/final.wav',
        what: "Render the full session to WAV, FLAC or MP3. FLAC is lossless — identical audio, roughly half the size. MP3 is lossy but plays anywhere; bitrate defaults to 192 kbps. Takes title, artist, album, year and comment, written as Vorbis comments on FLAC and ID3v2 on MP3, and can carry the session's markers through as chapters.",
        output: "path, duration_sec, peak_dbfs, sample_rate, tagged, chapters",
        note: "Tags are applied after encoding — FLAC blocks are rewritten in place and an ID3 tag is a prefix — so tagging never touches a sample. WAV has no standard tag container worth using and refuses metadata rather than dropping it silently.",
      },
      {
        name: "render_preview",
        prompt: "(used internally for playback)",
        what: "Render a preview WAV, cached by node id — re-previewing the same session state reuses the render instead of redoing it.",
        output: "path",
      },
      {
        name: "export_multiple",
        prompt: "export tracks 1 and 2 as separate files",
        what: "Export selected tracks as individual WAV files to a directory.",
        output: "list of exported paths",
      }
    ],
  },
];

export default function ToolsPage() {
  return (
    <DocShell
      title="Audio Tools Reference"
      description="All 91 tools the AI agent can call to edit your audio session."
    >
      <p>
        Tools are deterministic functions the agent calls to manipulate your
        audio session. You do not invoke tools directly — instead, describe what
        you want in natural language and the agent selects the right tool chain.
        Every tool call creates a new session node (non-destructive).
      </p>

      <h2>Prompt tips</h2>
      <ul>
        <li>
          <strong>Name the track</strong> when you have multiple:{" "}
          <code>normalize track 1</code> not just <code>normalize</code>.
        </li>
        <li>
          <strong>Use minutes:seconds</strong> for time:{" "}
          <code>cut from 1:30 to 2:00</code>.
        </li>
        <li>
          <strong>Chain operations</strong> in one message — the agent plans the
          full sequence before executing.
        </li>
        <li>
          <strong>Correct inline</strong> — if the agent misunderstood, say what
          was wrong: <code>not that track — the second one</code>.
        </li>
      </ul>

      {groups.map((group) => (
        <div key={group.title}>
          <h2>{group.title}</h2>
          <div className="not-prose space-y-4">
            {group.tools.map((tool) => (
              <div
                key={tool.name}
                className="rounded-lg border border-border/50 bg-card/40 p-4"
              >
                <div className="flex flex-wrap items-start gap-2">
                  <code className="rounded bg-primary/10 px-2 py-0.5 text-sm font-mono text-primary">
                    {tool.name}
                  </code>
                </div>
                <p className="mt-2 text-sm leading-relaxed text-foreground/90">
                  {tool.what}
                </p>
                <p className="mt-2 text-xs text-muted-foreground">
                  <span className="font-medium text-foreground/70">Example prompt: </span>
                  <code className="rounded bg-secondary px-1.5 py-0.5 font-mono">
                    {tool.prompt}
                  </code>
                </p>
                <p className="mt-1 text-xs text-muted-foreground">
                  <span className="font-medium text-foreground/70">Returns: </span>
                  {tool.output}
                </p>
                {tool.note && (
                  <p className="mt-2 rounded bg-primary/5 px-3 py-2 text-xs text-foreground/80 border border-primary/20">
                    {tool.note}
                  </p>
                )}
              </div>
            ))}
          </div>
        </div>
      ))}
    </DocShell>
  );
}
