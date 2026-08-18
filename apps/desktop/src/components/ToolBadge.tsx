/**
 * ToolBadge — compact pill rendering a tool-call's lifecycle (Studio Onyx).
 *
 * Visual states use semantic accent tokens (warning/success/danger)
 * mapped to the design system, so the dark theme stays cohesive
 * across the whole app rather than each component picking its own
 * red/green/yellow.
 */

import type { ToolStatus } from "../hooks/useAgentStream";

export interface ToolBadgeProps {
  name: string;
  status: ToolStatus;
  result?: string;
}

/**
 * Display name for every tool the dispatcher registers.
 *
 * Must stay exactly in step with the registered set — no key without a
 * tool, no tool without a key. `tool_badge_labels.rs` in `crates/tools`
 * enforces both directions, because this map had silently drifted:
 * nine entries named tools that had been renamed away (`cut` →
 * `cut_range`, `fade_in`/`fade_out` → `fade`, `set_volume` →
 * `set_track_gain`, …) and 49 of the 69 real tools had no entry at all,
 * falling through to the underscore-stripping fallback and rendering as
 * "de esser" and "high pass filter".
 */
const TOOL_LABELS: Record<string, string> = {
  // Session and I/O
  load: "Load audio",
  render_preview: "Preview",
  render_final: "Export",
  export_multiple: "Export tracks",
  export_labels: "Export labels",
  import_labels: "Import labels",

  // Analysis (read-only)
  analyze_track: "Analyze audio",
  plot_spectrum: "Spectrum",
  silence_finder: "Find silence",
  transcribe: "Transcribe",
  separate_stems: "Separate stems",

  // History / session graph
  fork_node: "Fork session",
  apply_diff: "Apply diff",
  compare_nodes: "Compare versions",
  revert_to: "Revert",
  name_node: "Rename version",
  label: "Add marker",

  // Tracks
  add_track: "Add track",
  remove_track: "Remove track",
  rename_track: "Rename track",
  duplicate_track: "Duplicate track",
  mute_track: "Mute track",
  solo_track: "Solo track",
  set_track_gain: "Track volume",
  set_pan: "Pan",
  mix_to_new_track: "Mix to new track",

  // Buses
  create_bus: "Create bus",
  set_send: "Route to bus",
  remove_send: "Remove send",

  // Arrangement
  cut_range: "Cut",
  trim: "Trim",
  copy_region: "Copy",
  paste_region: "Paste",
  split_clip: "Split clip",
  storage_report: "Disk usage",
  audition_effect: "Audition effect",
  export_recipe: "Export recipe",
  apply_recipe: "Apply recipe",
  move_clip: "Move clip",
  remove_clip: "Remove clip",
  insert_silence: "Insert silence",
  silence_region: "Silence region",
  repeat_selection: "Repeat selection",
  time_shift: "Shift in time",
  truncate_silence: "Truncate silence",
  set_clip_envelope: "Volume envelope",
  align_to_beat: "Align to beat",

  // Level
  gain: "Gain",
  normalize: "Normalize",
  normalize_loudness: "Normalize loudness",
  leveler: "Leveler",
  compressor: "Compressor",
  limiter: "Limiter",
  noise_gate: "Noise gate",

  // Tone
  eq: "Equalizer",
  low_pass_filter: "Low-pass filter",
  high_pass_filter: "High-pass filter",
  notch_filter: "Notch filter",

  // Repair
  noise_reduction: "Noise reduction",
  click_removal: "Click removal",
  de_esser: "De-esser",
  vocal_reduction: "Vocal reduction",

  // Effect chain (non-destructive)
  add_effect: "Add effect",
  remove_effect: "Remove effect",
  reorder_effects: "Reorder effects",
  set_effect_params: "Effect settings",
  set_effect_bypassed: "Bypass effect",

  // Effects
  reverb: "Reverb",
  echo: "Echo",
  distortion: "Distortion",
  phaser: "Phaser",
  tremolo: "Tremolo",
  stereo_widener: "Stereo widener",

  // Time and pitch
  time_stretch: "Time stretch",
  pitch_shift: "Pitch shift",
  change_speed: "Change speed",

  // Sample-level
  reverse: "Reverse",
  invert: "Invert polarity",
  fade: "Fade",

  // Format
  resample_track: "Resample",
  mono_to_stereo: "Mono to stereo",
  stereo_to_mono: "Stereo to mono",

  // Generators
  generate_tone: "Generate tone",
  generate_noise: "Generate noise",
};

const STATUS_GLYPH: Record<ToolStatus, string> = {
  running: "⟳",
  ok: "✓",
  error: "✗",
};

const STATUS_CLASS: Record<ToolStatus, string> = {
  running:
    "border-[var(--warning)]/35 bg-[var(--warning)]/10 text-[var(--warning)]",
  ok: "border-[var(--success)]/35 bg-[var(--success)]/10 text-[var(--success)]",
  error:
    "border-[var(--danger)]/40 bg-[var(--danger)]/10 text-[var(--danger)]",
};

export function ToolBadge({ name, status, result }: ToolBadgeProps) {
  const friendlyName = TOOL_LABELS[name] ?? name.replace(/_/g, " ");
  const label =
    status === "running" ? `${friendlyName}…` : (result ?? friendlyName);
  return (
    <span
      data-testid="tool-badge"
      data-status={status}
      className={
        "inline-flex items-center gap-1.5 rounded-full border px-2.5 py-0.5 font-mono text-[10px] uppercase tracking-wider " +
        STATUS_CLASS[status]
      }
    >
      <span
        aria-hidden="true"
        className={status === "running" ? "animate-spin" : ""}
      >
        {STATUS_GLYPH[status]}
      </span>
      <span className="normal-case tracking-normal">{label}</span>
    </span>
  );
}
