/**
 * CommandPalette — Ctrl+K searchable launcher for the agent's tools.
 *
 * Opens as a modal overlay. User types to filter, clicks or presses Enter
 * to select — the chosen prompt is injected into the chat input so the
 * user can review/edit before sending.
 *
 * Commands are grouped by category, with natural-language prompts that the
 * agent understands. A command here is a promise to the user that something
 * will happen, so a tool that records state without changing the audio must
 * not appear — see the note on the Speed & Pitch group.
 */

import { useEffect, useMemo, useRef, useState } from "react";

export interface CommandPaletteProps {
  open: boolean;
  onClose: () => void;
  onSelect: (prompt: string) => void;
}

export interface Command {
  category: string;
  label: string;
  prompt: string;
  description: string;
  tags?: string[];
}

export const COMMANDS: Command[] = [
  // Volume
  // Volume
  { category: "Volume", label: "Make louder", prompt: "make this 6 dB louder", description: "Boost track gain by +6 dB", tags: ["gain", "amplify", "louder"] },
  { category: "Volume", label: "Make quieter", prompt: "make this 6 dB quieter", description: "Reduce track gain by -6 dB", tags: ["gain", "quiet", "lower"] },
  { category: "Volume", label: "Normalize", prompt: "normalize to -1 dBFS", description: "Set peak amplitude to target level", tags: ["normalize", "peak", "level"] },
  { category: "Volume", label: "Set track volume", prompt: "set track 1 volume to -12 dB", description: "Set absolute gain (not additive)", tags: ["volume", "gain", "dB"] },

  // Loudness targets by name (#169). The numbers live in the tool's
  // preset table, not here — a platform that moves its target moves it
  // in one place, and the user never has to know it.
  { category: "Volume", label: "Loudness: Spotify / YouTube", prompt: "normalize track 1 loudness using the spotify preset", description: "-14 LUFS, EBU R128 integrated", tags: ["loudness", "lufs", "spotify", "youtube", "streaming", "master"] },
  { category: "Volume", label: "Loudness: Apple Podcasts", prompt: "normalize track 1 loudness using the apple_podcasts preset", description: "-16 LUFS, EBU R128 integrated", tags: ["loudness", "lufs", "apple", "podcast", "master"] },
  { category: "Volume", label: "Loudness: Broadcast", prompt: "normalize track 1 loudness using the broadcast preset", description: "-23 LUFS, EBU R128 (broadcast)", tags: ["loudness", "lufs", "broadcast", "ebu", "r128", "master"] },

  // Music under voice (#168 §1). The headline reason to use this over
  // a sidechain compressor, and it was undiscoverable.
  { category: "Volume", label: "Duck music under speech", prompt: "duck the music on track 1 under the speech on track 0", description: "Keyed on the transcript, not on level — a breath won't trigger it", tags: ["duck", "ducking", "music", "sidechain", "under", "voice", "bed"] },

  // Fades
  // Fades
  { category: "Fades", label: "Fade in", prompt: "add a fade-in over the first 3 seconds", description: "Linear fade from silence", tags: ["fade", "intro", "ramp"] },
  { category: "Fades", label: "Fade out", prompt: "fade out the last 3 seconds", description: "Linear fade to silence", tags: ["fade", "outro", "end"] },

  // Effects
  // Effects
  { category: "Effects", label: "Reduce noise", prompt: "reduce the background noise — the first 0.5 seconds is silence I can use as a noise profile", description: "Spectral subtraction noise removal", tags: ["noise", "denoise", "hiss", "background", "clean"] },
  { category: "Effects", label: "EQ boost highs", prompt: "apply EQ: boost 3 dB at 8000 Hz and 2 dB at 12000 Hz", description: "Peak EQ biquad filter chain", tags: ["eq", "equalizer", "treble", "highs", "frequency", "boost"] },
  { category: "Effects", label: "EQ cut lows", prompt: "apply EQ: cut 6 dB at 80 Hz", description: "Peak EQ low-frequency cut", tags: ["eq", "equalizer", "bass", "lows", "frequency", "cut"] },
  { category: "Effects", label: "Compress dynamics", prompt: "compress the dynamic range: threshold -12 dB, ratio 4:1, attack 5 ms, release 100 ms", description: "Dynamic range compressor", tags: ["compress", "compressor", "dynamics", "loudness", "ratio"] },
  { category: "Effects", label: "Gentle compression", prompt: "apply gentle compression: threshold -18 dB, ratio 2:1, attack 10 ms, release 200 ms, makeup gain 3 dB", description: "Subtle levelling compressor", tags: ["compress", "compressor", "gentle", "level", "dynamics"] },
  { category: "Effects", label: "Add a live effect to a track", prompt: "add a low-pass filter at 4000 Hz to track 1's effect chain", description: "Non-destructive — applied at render, stays editable", tags: ["effect", "chain", "live", "non-destructive", "add"] },
  { category: "Effects", label: "Bypass a live effect", prompt: "bypass effect 0 on track 1", description: "A/B a chain effect without removing it", tags: ["effect", "chain", "bypass", "off", "ab"] },
  { category: "Effects", label: "Reorder a track's effects", prompt: "reorder track 1's effects so the EQ runs before the compressor", description: "Chain order changes the sound", tags: ["effect", "chain", "reorder", "order"] },
  { category: "Effects", label: "Remove a live effect", prompt: "remove effect 0 from track 1", description: "Delete an effect from the chain", tags: ["effect", "chain", "remove", "delete"] },

  // Repair — the tools people go looking for by symptom.
  { category: "Effects", label: "Tame harsh S sounds", prompt: "de-ess track 0", description: "Reduce sibilance on a voice", tags: ["deess", "de-esser", "sibilance", "harsh", "ess", "voice"] },
  { category: "Effects", label: "Remove clicks and pops", prompt: "remove the clicks on track 0", description: "Repair impulsive noise", tags: ["click", "pop", "crackle", "repair", "vinyl"] },
  { category: "Effects", label: "Gate the background", prompt: "apply a noise gate to track 0", description: "Silence what falls below a threshold", tags: ["gate", "noise", "background", "bleed"] },
  { category: "Effects", label: "Limit the peaks", prompt: "apply a limiter to track 0 at -1 dB", description: "Stop anything exceeding a ceiling", tags: ["limit", "limiter", "peak", "ceiling", "master"] },
  { category: "Effects", label: "Even out the level", prompt: "level track 0", description: "Smooth loud and quiet passages", tags: ["level", "leveler", "even", "consistent", "dynamics"] },
  { category: "Effects", label: "Add reverb", prompt: "add reverb to track 0", description: "Room and space", tags: ["reverb", "room", "space", "hall", "ambience"] },
  { category: "Effects", label: "Add echo", prompt: "add an echo to track 0 with a 300 ms delay", description: "Repeating delay", tags: ["echo", "delay", "repeat", "slap"] },
  { category: "Effects", label: "Widen the stereo image", prompt: "widen the stereo image on track 0", description: "Broaden a stereo track", tags: ["stereo", "widen", "width", "image", "spread"] },
  { category: "Effects", label: "Try an effect before applying", prompt: "audition a reverb on track 0 so I can hear it first", description: "Preview an effect without committing it", tags: ["audition", "preview", "try", "test", "before"] },

  // Editing
  // Editing
  { category: "Editing", label: "Cut region", prompt: "cut the selected region and close the gap", description: "Remove selection and close gap", tags: ["cut", "delete", "remove"] },
  { category: "Editing", label: "Trim to selection", prompt: "trim to keep only the selected region", description: "Discard everything outside selection", tags: ["trim", "crop", "keep"] },
  { category: "Editing", label: "Copy region", prompt: "copy the selected region to the clipboard", description: "Copy time range to clipboard", tags: ["copy", "clipboard"] },
  { category: "Editing", label: "Paste audio", prompt: "paste clipboard audio at 0 seconds", description: "Insert clipboard at time offset", tags: ["paste", "insert"] },
  { category: "Editing", label: "Insert silence", prompt: "insert 2 seconds of silence at 0 seconds", description: "Add blank space", tags: ["silence", "gap", "blank"] },
  { category: "Editing", label: "Reverse", prompt: "reverse the audio", description: "Flip sample order — plays backwards", tags: ["reverse", "backwards", "flip"] },

  // Podcast cleanup. These are the reason someone opens a voice editor
  // at all, and none of them were reachable here — the palette had
  // "Reverse" and "Pitch down" but no way to find silence removal.
  { category: "Editing", label: "Tighten the silences", prompt: "truncate the silences longer than 0.5 seconds down to 0.3 seconds", description: "Shorten long gaps without removing them", tags: ["silence", "truncate", "tighten", "gaps", "pauses", "pacing"] },
  { category: "Editing", label: "Remove filler words", prompt: "remove the filler words", description: "Cut um, uh and like using the transcript", tags: ["filler", "um", "uh", "like", "clean", "speech"] },
  { category: "Editing", label: "Cut specific words", prompt: "cut the words 'you know' everywhere they appear", description: "Cut by transcript text rather than by time", tags: ["cut", "words", "transcript", "text"] },
  { category: "Editing", label: "Find the silences", prompt: "find the silent stretches longer than a second", description: "Report silent ranges without changing anything", tags: ["silence", "find", "detect", "gaps"] },

  // Describing a region instead of dragging one (#168 §3). Worth
  // surfacing precisely because it makes every range-taking tool
  // reachable by description, which is invisible if you never learn
  // the feature exists.
  { category: "Editing", label: "Select by description", prompt: "select where he talks about latency", description: "Resolve a described region into a selection", tags: ["select", "selection", "describe", "find", "region", "transcript"] },
  { category: "Editing", label: "Select the last of the speech", prompt: "select the last thirty seconds of speech", description: "Resolve a speech passage into a selection", tags: ["select", "speech", "passage", "last", "end"] },
  { category: "Editing", label: "Select between beats", prompt: "select from beat 16 to beat 32", description: "Resolve a beat range into a selection", tags: ["select", "beat", "bar", "grid", "music"] },

  // Arrangement.
  { category: "Editing", label: "Repeat a selection", prompt: "repeat the selected region 4 times", description: "Loop a range in place", tags: ["repeat", "loop", "duplicate", "times"] },
  { category: "Editing", label: "Move a clip", prompt: "move clip 1 on track 0 to start at 12 seconds", description: "Reposition a clip on the timeline", tags: ["move", "clip", "shift", "position"] },
  { category: "Editing", label: "Split a clip", prompt: "split clip 0 on track 0 at 30 seconds", description: "Cut one clip into two at a point", tags: ["split", "clip", "divide", "cut"] },
  { category: "Editing", label: "Silence a region", prompt: "silence the selected region without closing the gap", description: "Mute a range in place, keeping the timing", tags: ["silence", "mute", "blank", "region"] },
  { category: "Editing", label: "Keep tracks in sync", prompt: "turn on sync lock so edits to one track shift the others too", description: "Time-shifting edits apply to every track", tags: ["sync", "lock", "multitrack", "interview"] },

  // Recording (#203 §2).
  { category: "Editing", label: "Punch in a retake", prompt: "punch in over the selected region on track 0", description: "Re-record a region in place, leaving the rest", tags: ["punch", "record", "retake", "fix", "line", "redo"] },

  // Speed & Pitch
  // Speed & Pitch
  //
  // `time_stretch` and `pitch_shift` apply a phase vocoder now. These
  // five were removed when they routed to tools that recorded a value
  // the render engine never read; they are back because the DSP landed.
  // `align_to_beat` is back too: it warps audio to a grid as of #97,
  // where before it recorded a grid nothing read.
  { category: "Speed & Pitch", label: "Slow down", prompt: "slow this down to 0.75x speed, keeping the pitch", description: "Time stretch — duration changes, pitch does not", tags: ["slow", "stretch", "tempo"] },
  { category: "Speed & Pitch", label: "Speed up", prompt: "speed this up to 1.5x, keeping the pitch", description: "Time compress — duration changes, pitch does not", tags: ["speed", "stretch", "tempo", "fast"] },
  { category: "Speed & Pitch", label: "Change speed (pitch follows)", prompt: "speed this up to 1.5x by resampling", description: "Resamples — pitch rises with the speed", tags: ["speed", "resample", "tempo"] },
  { category: "Speed & Pitch", label: "Pitch up", prompt: "pitch shift up 2 semitones", description: "Raise pitch, duration unchanged", tags: ["pitch", "semitone", "higher"] },
  { category: "Speed & Pitch", label: "Pitch down", prompt: "pitch shift down 3 semitones", description: "Lower pitch, duration unchanged", tags: ["pitch", "semitone", "lower"] },
  { category: "Speed & Pitch", label: "Align to a steady beat", prompt: "analyze this to find the beats, then warp it so they land on a steady grid at the same average tempo", description: "Warps timing onto a beat grid — pitch unchanged", tags: ["beat", "grid", "align", "quantize", "timing", "warp"] },

  // Analysis
  // Analysis
  { category: "Analysis", label: "Analyze audio", prompt: "analyze this audio — give me the BPM, key, loudness, and sections", description: "BPM, key, LUFS, beat grid, sections", tags: ["analyze", "bpm", "key", "loudness", "lufs"] },
  { category: "Analysis", label: "Transcribe speech", prompt: "transcribe this audio", description: "Speech to text via local Whisper model", tags: ["transcribe", "speech", "text", "whisper"] },
  { category: "Analysis", label: "Separate stems", prompt: "separate this into vocals, drums, bass, and other stems", description: "Demucs stem separation", tags: ["stems", "demucs", "vocals", "drums", "bass", "separate"] },

  // Tracks
  // Tracks
  { category: "Tracks", label: "Add track", prompt: "add a new empty track", description: "Append a silent track to the session", tags: ["add", "track", "new"] },
  { category: "Tracks", label: "Remove track", prompt: "remove track 1", description: "Delete a track from the session", tags: ["remove", "delete", "track"] },
  { category: "Tracks", label: "Mute track", prompt: "mute track 1", description: "Silence a specific track", tags: ["mute", "silence", "track"] },
  { category: "Tracks", label: "Load audio file", prompt: "load a new audio file as track 2", description: "Decode file and add as new track", tags: ["load", "import", "file", "track"] },

  // Interviews.
  { category: "Tracks", label: "Split by speaker", prompt: "split track 0 by speaker", description: "One track per voice, so each can be mixed on its own", tags: ["speaker", "diarize", "interview", "split", "voices", "podcast"] },
  { category: "Tracks", label: "Solo a track", prompt: "solo track 1", description: "Hear one track on its own", tags: ["solo", "isolate", "listen"] },
  { category: "Tracks", label: "Pan a track", prompt: "pan track 1 hard left", description: "Position a track in the stereo field", tags: ["pan", "stereo", "left", "right", "width"] },
  { category: "Tracks", label: "Duplicate a track", prompt: "duplicate track 0", description: "Copy a track and its clips", tags: ["duplicate", "copy", "clone"] },
  { category: "Tracks", label: "Mix tracks together", prompt: "mix tracks 0 and 1 into a new track", description: "Render selected tracks down to one", tags: ["mix", "bounce", "combine", "flatten"] },
  { category: "Tracks", label: "Send tracks to a bus", prompt: "create a reverb bus and send track 1 to it at -6 dB", description: "Shared processing for several tracks", tags: ["bus", "send", "aux", "routing", "group"] },

  // Generators and format.
  { category: "Tracks", label: "Generate a tone", prompt: "generate a 440 Hz tone for 5 seconds", description: "A reference tone as a new track", tags: ["tone", "sine", "generate", "test", "reference"] },
  { category: "Tracks", label: "Generate noise", prompt: "generate 5 seconds of pink noise", description: "White or pink noise as a new track", tags: ["noise", "white", "pink", "generate", "test"] },
  { category: "Tracks", label: "Change the sample rate", prompt: "resample track 0 to 44100 Hz", description: "Convert a track's sample rate", tags: ["resample", "rate", "44100", "48000", "convert"] },
  { category: "Tracks", label: "Mono to stereo", prompt: "convert track 0 from mono to stereo", description: "Widen a mono track to two channels", tags: ["mono", "stereo", "channels", "convert"] },
  { category: "Tracks", label: "Stereo to mono", prompt: "convert track 0 from stereo to mono", description: "Fold a stereo track to one channel", tags: ["stereo", "mono", "channels", "fold", "convert"] },

  // Export & Session
  // Export & Session
  { category: "Export & Session", label: "Export to WAV", prompt: "export the final mix to a WAV file", description: "Render and save to disk", tags: ["export", "render", "wav", "save"] },
  { category: "Export & Session", label: "Export selection", prompt: "export only the selected region to a WAV file", description: "Render a time range to disk", tags: ["export", "selection", "region", "wav"] },
  { category: "Export & Session", label: "Preview current state", prompt: "render a quick preview so I can hear the current state", description: "Render to temp file (no new node)", tags: ["preview", "render", "listen"] },
  { category: "Export & Session", label: "Add marker", prompt: "add a marker called chorus at the current position", description: "Place a named timeline marker", tags: ["marker", "label", "annotate"] },
  { category: "Export & Session", label: "Undo last change", prompt: "undo the last change", description: "Revert head to parent node", tags: ["undo", "revert", "back"] },
  { category: "Export & Session", label: "Compare two versions", prompt: "compare this version with the previous one", description: "Side-by-side A/B node diff", tags: ["compare", "diff", "ab", "versions"] },
  { category: "Export & Session", label: "Fork session", prompt: "fork the session here so I can try something different without losing the original", description: "Branch the session DAG", tags: ["fork", "branch", "alternative", "dag"] },
  { category: "Export & Session", label: "Rename node", prompt: "name this version 'rough mix'", description: "Set a label on the current node", tags: ["name", "label", "rename", "node"] },

  // Disk (#98). storage_report measures, compact_session reclaims —
  // pairing them here so the report is not a dead end.
  { category: "Export & Session", label: "Check disk usage", prompt: "how much disk is this session using?", description: "Break down what the session costs on disk", tags: ["disk", "size", "storage", "space", "usage"] },
  { category: "Export & Session", label: "Reclaim disk space", prompt: "compact the session to reclaim disk space, keeping the last 20 versions", description: "Prune old history and sweep the audio it held", tags: ["compact", "reclaim", "prune", "cleanup", "disk", "space"] },

  // Labels and repeatability.
  { category: "Export & Session", label: "Export markers", prompt: "export the labels", description: "Write markers out as a label file", tags: ["labels", "markers", "export", "chapters"] },
  { category: "Export & Session", label: "Import markers", prompt: "import labels from a file", description: "Read markers in from a label file", tags: ["labels", "markers", "import", "chapters"] },
  { category: "Export & Session", label: "Save these steps as a recipe", prompt: "export what I just did as a recipe", description: "Capture the edit chain for reuse", tags: ["recipe", "export", "save", "reuse", "chain"] },
  { category: "Export & Session", label: "Apply a saved recipe", prompt: "apply the podcast recipe to this file", description: "Replay a saved edit chain", tags: ["recipe", "apply", "replay", "reuse"] },
  { category: "Export & Session", label: "Run the same edits on many files", prompt: "apply this chain to every file in the folder", description: "Batch a chain across a set of files", tags: ["batch", "bulk", "many", "folder", "apply"] },
  { category: "Export & Session", label: "Export each track separately", prompt: "export each track as its own file", description: "One file per track rather than a mix", tags: ["export", "stems", "separate", "tracks", "multitrack"] },
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
      className="backdrop-in fixed inset-0 z-50 flex items-start justify-center bg-black/60 pt-[15vh]"
      onClick={handleBackdrop}
    >
      <div
        className="overlay-in flex w-full max-w-xl flex-col overflow-hidden rounded-xl border border-[var(--border-strong)] bg-[var(--surface-elev)] shadow-[0_24px_60px_-12px_rgba(0,0,0,0.8)]"
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
