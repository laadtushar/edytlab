import type { Metadata } from "next";
import { siteConfig } from "@/lib/site";
import { DocShell } from "@/components/docs/doc-shell";

export const metadata: Metadata = {
  title: "Audio Tools Reference",
  description:
    "All 33 audio-editing tools available to the edytlab AI agent — cut, normalize, stem separate, transcribe, render, and more.",
  alternates: { canonical: "/docs/tools" },
  openGraph: {
    title: "Audio Tools Reference — edytlab Docs",
    description: "Complete reference for all 33 agent-callable audio tools.",
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
        name: "normalize",
        prompt: 'normalize to -14 LUFS for Spotify',
        what: "Normalize a track to an integrated LUFS target or true peak limit.",
        output: "node_id, applied_gain_db",
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
        what: "Change the pitch without changing the duration. Range: −12 to +12 semitones.",
        output: "node_id",
      },
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
        prompt: 'align track 2 to the beat',
        what: "Shift the start of a track to align with the nearest beat grid.",
        output: "node_id, shift_sec",
      },
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
    ],
  },
  {
    title: "Rendering",
    tools: [
      {
        name: "render_final",
        prompt: 'export to /Users/me/Desktop/final.wav',
        what: "Render the full session to a WAV file at 16, 24, or 32-bit depth.",
        output: "path, duration_sec, peak_dbfs, sample_rate",
      },
      {
        name: "render_preview",
        prompt: "(used internally for playback)",
        what: "Render a preview WAV to a temp file. Valid for the current app session.",
        output: "path",
      },
    ],
  },
];

export default function ToolsPage() {
  return (
    <DocShell
      title="Audio Tools Reference"
      description="All 33 tools the AI agent can call to edit your audio session."
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
