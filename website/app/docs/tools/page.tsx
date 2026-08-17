import type { Metadata } from "next";
import { siteConfig } from "@/lib/site";
import { DocShell } from "@/components/docs/doc-shell";

export const metadata: Metadata = {
  title: "Audio Tools Reference",
  description:
    "All 81 audio-editing tools available to the edytlab AI agent — cut, normalize, stem separate, transcribe, render, and more.",
  alternates: { canonical: "/docs/tools" },
  openGraph: {
    title: "Audio Tools Reference — edytlab Docs",
    description: "Complete reference for all 81 agent-callable audio tools.",
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
        output: "node_id",
      },
      {
        name: "copy_region",
        prompt: 'copy the section from 0:30 to 1:00',
        what: "Copy a time region to the clipboard.",
        output: "duration_sec of copied region",
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
        prompt: 'normalize to -14 LUFS for Spotify',
        what: "Set gain so the track hits an integrated LUFS target, measured with EBU R128. Gain is capped at a true-peak ceiling (−1 dBFS by default) so it never clips getting there; when the cap bites, the result reports the shortfall rather than claiming success.",
        output: "node_id, measured_lufs, applied_gain_db, achieved_lufs, shortfall_db, capped_by_ceiling",
        note: "Common targets: −14 LUFS Spotify/YouTube, −16 LUFS Apple Podcasts, −23 LUFS broadcast.",
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
        name: "storage_report",
        prompt: "how much disk is this session using?",
        what: "Report what the session costs on disk, split three ways: audio the current version needs, audio only the undo history needs, and audio nothing references at all. Every destructive edit writes a new file and none are deleted, so a long session grows without bound. Reads only — it deletes nothing.",
        output: "total_bytes, live, history, unreferenced, largest_unreferenced",
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
        what: "Render the full session to WAV, FLAC or MP3. FLAC is lossless — identical audio, roughly half the size. MP3 is lossy but plays anywhere; bitrate defaults to 192 kbps.",
        output: "path, duration_sec, peak_dbfs, sample_rate",
      },
      {
        name: "render_preview",
        prompt: "(used internally for playback)",
        what: "Render a preview WAV to a temp file. Valid for the current app session.",
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
      description="All 81 tools the AI agent can call to edit your audio session."
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
