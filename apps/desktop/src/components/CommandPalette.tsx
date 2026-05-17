/**
 * CommandPalette — Ctrl+K searchable launcher for all 27 agent tools.
 *
 * Opens as a modal overlay. User types to filter, clicks or presses Enter
 * to select — the chosen prompt is injected into the chat input so the
 * user can review/edit before sending.
 *
 * Commands are grouped by category and cover every tool the Rust dispatcher
 * exposes, with natural-language prompts that the agent understands.
 */

import { useEffect, useMemo, useRef, useState } from "react";

export interface CommandPaletteProps {
  open: boolean;
  onClose: () => void;
  onSelect: (prompt: string) => void;
}

interface Command {
  category: string;
  label: string;
  prompt: string;
  description: string;
  tags?: string[];
}

const COMMANDS: Command[] = [
  // Volume
  { category: "Volume", label: "Make louder", prompt: "make this 6 dB louder", description: "Boost track gain by +6 dB", tags: ["gain", "amplify", "louder"] },
  { category: "Volume", label: "Make quieter", prompt: "make this 6 dB quieter", description: "Reduce track gain by -6 dB", tags: ["gain", "quiet", "lower"] },
  { category: "Volume", label: "Normalize", prompt: "normalize to -1 dBFS", description: "Set peak amplitude to target level", tags: ["normalize", "peak", "level"] },
  { category: "Volume", label: "Set track volume", prompt: "set track 1 volume to -12 dB", description: "Set absolute gain (not additive)", tags: ["volume", "gain", "dB"] },

  // Fades
  { category: "Fades", label: "Fade in", prompt: "add a fade-in over the first 3 seconds", description: "Linear fade from silence", tags: ["fade", "intro", "ramp"] },
  { category: "Fades", label: "Fade out", prompt: "fade out the last 3 seconds", description: "Linear fade to silence", tags: ["fade", "outro", "end"] },

  // Effects
  { category: "Effects", label: "Reduce noise", prompt: "reduce the background noise — the first 0.5 seconds is silence I can use as a noise profile", description: "Spectral subtraction noise removal", tags: ["noise", "denoise", "hiss", "background", "clean"] },
  { category: "Effects", label: "EQ boost highs", prompt: "apply EQ: boost 3 dB at 8000 Hz and 2 dB at 12000 Hz", description: "Peak EQ biquad filter chain", tags: ["eq", "equalizer", "treble", "highs", "frequency", "boost"] },
  { category: "Effects", label: "EQ cut lows", prompt: "apply EQ: cut 6 dB at 80 Hz", description: "Peak EQ low-frequency cut", tags: ["eq", "equalizer", "bass", "lows", "frequency", "cut"] },
  { category: "Effects", label: "Compress dynamics", prompt: "compress the dynamic range: threshold -12 dB, ratio 4:1, attack 5 ms, release 100 ms", description: "Dynamic range compressor", tags: ["compress", "compressor", "dynamics", "loudness", "ratio"] },
  { category: "Effects", label: "Gentle compression", prompt: "apply gentle compression: threshold -18 dB, ratio 2:1, attack 10 ms, release 200 ms, makeup gain 3 dB", description: "Subtle levelling compressor", tags: ["compress", "compressor", "gentle", "level", "dynamics"] },

  // Editing
  { category: "Editing", label: "Cut region", prompt: "cut the selected region and close the gap", description: "Remove selection and close gap", tags: ["cut", "delete", "remove"] },
  { category: "Editing", label: "Trim to selection", prompt: "trim to keep only the selected region", description: "Discard everything outside selection", tags: ["trim", "crop", "keep"] },
  { category: "Editing", label: "Copy region", prompt: "copy the selected region to the clipboard", description: "Copy time range to clipboard", tags: ["copy", "clipboard"] },
  { category: "Editing", label: "Paste audio", prompt: "paste clipboard audio at 0 seconds", description: "Insert clipboard at time offset", tags: ["paste", "insert"] },
  { category: "Editing", label: "Insert silence", prompt: "insert 2 seconds of silence at 0 seconds", description: "Add blank space", tags: ["silence", "gap", "blank"] },
  { category: "Editing", label: "Reverse", prompt: "reverse the audio", description: "Flip sample order — plays backwards", tags: ["reverse", "backwards", "flip"] },

  // Speed & Pitch
  { category: "Speed & Pitch", label: "Slow down", prompt: "slow this down to 0.75x speed", description: "Time stretch without pitch change", tags: ["slow", "stretch", "tempo"] },
  { category: "Speed & Pitch", label: "Speed up", prompt: "speed this up to 1.5x", description: "Time compress without pitch change", tags: ["speed", "compress", "tempo", "fast"] },
  { category: "Speed & Pitch", label: "Pitch up", prompt: "pitch shift up 2 semitones", description: "Raise pitch, keep duration", tags: ["pitch", "semitone", "higher"] },
  { category: "Speed & Pitch", label: "Pitch down", prompt: "pitch shift down 3 semitones", description: "Lower pitch, keep duration", tags: ["pitch", "semitone", "lower"] },
  { category: "Speed & Pitch", label: "Align to beat", prompt: "align this track to the beat grid", description: "Quantize clip positions to detected BPM", tags: ["beat", "align", "quantize", "grid"] },

  // Analysis
  { category: "Analysis", label: "Analyze audio", prompt: "analyze this audio — give me the BPM, key, loudness, and sections", description: "BPM, key, LUFS, beat grid, sections", tags: ["analyze", "bpm", "key", "loudness", "lufs"] },
  { category: "Analysis", label: "Transcribe speech", prompt: "transcribe this audio", description: "Speech to text via local Whisper model", tags: ["transcribe", "speech", "text", "whisper"] },
  { category: "Analysis", label: "Separate stems", prompt: "separate this into vocals, drums, bass, and other stems", description: "Demucs stem separation", tags: ["stems", "demucs", "vocals", "drums", "bass", "separate"] },

  // Tracks
  { category: "Tracks", label: "Add track", prompt: "add a new empty track", description: "Append a silent track to the session", tags: ["add", "track", "new"] },
  { category: "Tracks", label: "Remove track", prompt: "remove track 1", description: "Delete a track from the session", tags: ["remove", "delete", "track"] },
  { category: "Tracks", label: "Mute track", prompt: "mute track 1", description: "Silence a specific track", tags: ["mute", "silence", "track"] },
  { category: "Tracks", label: "Load audio file", prompt: "load a new audio file as track 2", description: "Decode file and add as new track", tags: ["load", "import", "file", "track"] },

  // Export & Session
  { category: "Export & Session", label: "Export to WAV", prompt: "export the final mix to a WAV file", description: "Render and save to disk", tags: ["export", "render", "wav", "save"] },
  { category: "Export & Session", label: "Export selection", prompt: "export only the selected region to a WAV file", description: "Render a time range to disk", tags: ["export", "selection", "region", "wav"] },
  { category: "Export & Session", label: "Preview current state", prompt: "render a quick preview so I can hear the current state", description: "Render to temp file (no new node)", tags: ["preview", "render", "listen"] },
  { category: "Export & Session", label: "Add marker", prompt: "add a marker called chorus at the current position", description: "Place a named timeline marker", tags: ["marker", "label", "annotate"] },
  { category: "Export & Session", label: "Undo last change", prompt: "undo the last change", description: "Revert head to parent node", tags: ["undo", "revert", "back"] },
  { category: "Export & Session", label: "Compare two versions", prompt: "compare this version with the previous one", description: "Side-by-side A/B node diff", tags: ["compare", "diff", "ab", "versions"] },
  { category: "Export & Session", label: "Fork session", prompt: "fork the session here so I can try something different without losing the original", description: "Branch the session DAG", tags: ["fork", "branch", "alternative", "dag"] },
  { category: "Export & Session", label: "Rename node", prompt: "name this version 'rough mix'", description: "Set a label on the current node", tags: ["name", "label", "rename", "node"] },
];

const CATEGORY_ORDER = [
  "Volume",
  "Fades",
  "Effects",
  "Editing",
  "Speed & Pitch",
  "Analysis",
  "Tracks",
  "Export & Session",
];

export function CommandPalette({ open, onClose, onSelect }: CommandPaletteProps) {
  const [query, setQuery] = useState("");
  const [activeIdx, setActiveIdx] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (open) {
      setQuery("");
      setActiveIdx(0);
      setTimeout(() => inputRef.current?.focus(), 0);
    }
  }, [open]);

  const filtered = useMemo(() => {
    const q = query.toLowerCase().trim();
    if (!q) return COMMANDS;
    return COMMANDS.filter(
      (c) =>
        c.label.toLowerCase().includes(q) ||
        c.description.toLowerCase().includes(q) ||
        c.category.toLowerCase().includes(q) ||
        (c.tags ?? []).some((t) => t.includes(q)),
    );
  }, [query]);

  const grouped = useMemo(() => {
    const map = new Map<string, Command[]>();
    for (const cmd of filtered) {
      const bucket = map.get(cmd.category) ?? [];
      bucket.push(cmd);
      map.set(cmd.category, bucket);
    }
    const out: { category: string; commands: Command[] }[] = [];
    for (const cat of CATEGORY_ORDER) {
      const cmds = map.get(cat);
      if (cmds && cmds.length > 0) out.push({ category: cat, commands: cmds });
    }
    return out;
  }, [filtered]);

  const flatFiltered = useMemo(() => filtered, [filtered]);

  useEffect(() => {
    setActiveIdx(0);
  }, [query]);

  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") { onClose(); return; }
      if (e.key === "ArrowDown") {
        e.preventDefault();
        setActiveIdx((i) => Math.min(i + 1, flatFiltered.length - 1));
      }
      if (e.key === "ArrowUp") {
        e.preventDefault();
        setActiveIdx((i) => Math.max(i - 1, 0));
      }
      if (e.key === "Enter" && flatFiltered[activeIdx]) {
        e.preventDefault();
        onSelect(flatFiltered[activeIdx].prompt);
        onClose();
      }
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [open, onClose, onSelect, flatFiltered, activeIdx]);

  if (!open) return null;

  const handleBackdrop = (e: React.MouseEvent) => {
    if (e.target === e.currentTarget) onClose();
  };

  let flatIdx = -1;

  return (
    <div
      className="fixed inset-0 z-50 flex items-start justify-center bg-black/60 pt-[15vh]"
      onClick={handleBackdrop}
    >
      <div
        className="flex w-full max-w-xl flex-col overflow-hidden rounded-xl border border-[var(--border-strong)] bg-[var(--surface-elev)] shadow-[0_24px_60px_-12px_rgba(0,0,0,0.8)]"
        style={{ maxHeight: "60vh" }}
      >
        {/* Search input */}
        <div className="flex items-center gap-3 border-b border-[var(--border)] px-4 py-3">
          <svg
            width="15"
            height="15"
            viewBox="0 0 15 15"
            fill="none"
            stroke="currentColor"
            strokeWidth="1.6"
            strokeLinecap="round"
            className="shrink-0 text-[var(--text-faint)]"
            aria-hidden="true"
          >
            <circle cx="6.5" cy="6.5" r="4.5" />
            <path d="M10.5 10.5l3 3" />
          </svg>
          <input
            ref={inputRef}
            type="text"
            placeholder="Search tools and commands..."
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            className="flex-1 bg-transparent text-sm text-[var(--text)] outline-none placeholder:text-[var(--text-faint)]"
            aria-label="Search commands"
          />
          <kbd className="rounded border border-[var(--border-strong)] bg-[var(--surface-elev-2)] px-1.5 py-0.5 font-mono text-[10px] text-[var(--text-faint)]">
            Esc
          </kbd>
        </div>

        {/* Results */}
        <div ref={listRef} className="flex-1 overflow-y-auto p-2">
          {grouped.length === 0 ? (
            <p className="px-3 py-6 text-center text-sm text-[var(--text-faint)]">
              No commands match &quot;{query}&quot;
            </p>
          ) : (
            grouped.map(({ category, commands }) => (
              <div key={category} className="mb-1">
                <div className="px-3 pb-0.5 pt-2">
                  <span className="font-mono text-[9px] uppercase tracking-[0.2em] text-[var(--text-faint)]">
                    {category}
                  </span>
                </div>
                {commands.map((cmd) => {
                  flatIdx += 1;
                  const thisIdx = flatIdx;
                  const isActive = thisIdx === activeIdx;
                  return (
                    <button
                      key={cmd.label}
                      type="button"
                      onClick={() => { onSelect(cmd.prompt); onClose(); }}
                      onMouseEnter={() => setActiveIdx(thisIdx)}
                      className={[
                        "flex w-full items-center gap-3 rounded-md px-3 py-2 text-left transition",
                        isActive
                          ? "bg-[var(--accent-soft)] text-[var(--text)]"
                          : "text-[var(--text-dim)] hover:bg-[var(--surface-elev-2)]",
                      ].join(" ")}
                    >
                      <span className="flex-1">
                        <span className="block text-sm font-medium leading-tight">
                          {cmd.label}
                        </span>
                        <span className="block text-[11px] leading-snug text-[var(--text-faint)]">
                          {cmd.description}
                        </span>
                      </span>
                      {isActive ? (
                        <span className="shrink-0 font-mono text-[10px] text-[var(--accent)]">
                          Enter
                        </span>
                      ) : null}
                    </button>
                  );
                })}
              </div>
            ))
          )}
        </div>

        {/* Footer */}
        <div className="flex items-center justify-between border-t border-[var(--border)] px-4 py-2">
          <span className="font-mono text-[9px] uppercase tracking-[0.18em] text-[var(--text-faint)]">
            {filtered.length} command{filtered.length !== 1 ? "s" : ""}
          </span>
          <div className="flex items-center gap-3 font-mono text-[9px] uppercase tracking-[0.18em] text-[var(--text-faint)]">
            <span>
              <kbd className="mr-1 rounded border border-[var(--border-strong)] bg-[var(--surface-elev-2)] px-1">↑↓</kbd>
              navigate
            </span>
            <span>
              <kbd className="mr-1 rounded border border-[var(--border-strong)] bg-[var(--surface-elev-2)] px-1">↵</kbd>
              fill chat
            </span>
          </div>
        </div>
      </div>
    </div>
  );
}
