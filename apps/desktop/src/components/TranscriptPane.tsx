/**
 * The transcript, as something you edit rather than something you read
 * (#157).
 *
 * `transcribe` has stored word-level timings in the session since it
 * shipped, and nothing surfaced them. This is the pane that does, and
 * the reason it matters is the sentence in the ticket: delete a
 * sentence in the text and the audio is cut to match. That is the whole
 * differentiator, and every piece of it below the UI already existed.
 *
 * ## Selection is the bridge
 *
 * A word span is a time range, so selecting text *is* selecting audio.
 * The pane reports its span in seconds and the timeline highlights it;
 * the timeline reports a drag and the matching words light up. Neither
 * side owns a second notion of "what is selected" — there is one range,
 * expressed in seconds, and two views of it.
 *
 * ## The edit goes through the tool
 *
 * Deleting calls `cut_words`, the same tool the agent calls. Not a
 * second implementation that happens to do the same thing: one node,
 * the same provenance, the same shifting of the remaining timings. Two
 * paths into one edit is how they drift.
 */

import { useEffect, useMemo, useRef, useState } from "react";

import type { TranscriptWord } from "../lib/tauri-bridge";

export interface TranscriptPaneProps {
  words: TranscriptWord[];
  /** Seconds range selected elsewhere (the timeline), if any. */
  selection?: { start: number; end: number } | null;
  /** Report a text selection as a time range, for the waveform. */
  onSelectRange?: (range: { start: number; end: number } | null) => void;
  /** Delete `[from, to)` — wired to `cut_words`. */
  onCutWords?: (from: number, to: number) => void;
  onSeek?: (timeSec: number) => void;
  /** True while a transcribe run is in flight. */
  busy?: boolean;
}

export function TranscriptPane({
  words,
  selection,
  onSelectRange,
  onCutWords,
  onSeek,
  busy = false,
}: TranscriptPaneProps) {
  // Anchor and focus of a drag, as word indices. Kept separate from the
  // derived range so a backwards drag works without the caller ever
  // seeing an inverted range.
  const [anchor, setAnchor] = useState<number | null>(null);
  const [focus, setFocus] = useState<number | null>(null);
  const dragging = useRef(false);

  const span = useMemo(() => {
    if (anchor === null || focus === null) return null;
    return { from: Math.min(anchor, focus), to: Math.max(anchor, focus) + 1 };
  }, [anchor, focus]);

  // Which words the *timeline's* selection covers, so a drag on the
  // waveform lights up the text. A word counts as covered when it
  // overlaps the range at all — a range clipped mid-word still means
  // that word.
  const coveredByTime = useMemo(() => {
    if (!selection) return null;
    const first = words.findIndex((w) => w.end_sec > selection.start);
    if (first === -1) return null;
    let last = first;
    for (let i = first; i < words.length; i++) {
      if (words[i].start_sec >= selection.end) break;
      last = i;
    }
    return { from: first, to: last + 1 };
  }, [selection, words]);

  const highlight = span ?? coveredByTime;

  // Report the text selection outward, in seconds.
  useEffect(() => {
    if (!span || !onSelectRange) return;
    const from = words[span.from];
    const to = words[span.to - 1];
    if (!from || !to) return;
    // The span runs from the first word's *start* to the last word's
    // *end* — cutting from the first word's end would leave a clipped
    // syllable behind, which is the same reasoning `cut_words` uses.
    onSelectRange({ start: from.start_sec, end: to.end_sec });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [span, words]);

  useEffect(() => {
    const up = () => {
      dragging.current = false;
    };
    window.addEventListener("mouseup", up);
    return () => window.removeEventListener("mouseup", up);
  }, []);

  function handleKeyDown(e: React.KeyboardEvent) {
    if ((e.key === "Backspace" || e.key === "Delete") && span && onCutWords) {
      e.preventDefault();
      onCutWords(span.from, span.to);
      setAnchor(null);
      setFocus(null);
    }
    if (e.key === "Escape") {
      setAnchor(null);
      setFocus(null);
      onSelectRange?.(null);
    }
  }

  if (busy) {
    return (
      <Empty>
        <span data-testid="transcript-busy">Transcribing…</span>
      </Empty>
    );
  }

  if (words.length === 0) {
    // An ordinary state, not a fault: say what to do rather than
    // showing an empty box that looks broken.
    return (
      <Empty>
        <p data-testid="transcript-empty" style={{ maxWidth: 420, lineHeight: 1.6 }}>
          No transcript for this session yet. Ask the agent to{" "}
          <code style={{ color: "var(--accent)" }}>transcribe</code> a track and the
          words will appear here — then you can cut the audio by deleting text.
        </p>
      </Empty>
    );
  }

  return (
    <div
      data-testid="transcript-pane"
      tabIndex={0}
      onKeyDown={handleKeyDown}
      style={{
        height: "100%",
        overflowY: "auto",
        padding: 20,
        lineHeight: 2.1,
        fontSize: 14,
        outline: "none",
      }}
    >
      <p
        style={{
          fontFamily: "var(--font-mono)",
          fontSize: 10,
          textTransform: "uppercase",
          letterSpacing: "0.08em",
          color: "var(--text-dim)",
          marginBottom: 12,
        }}
      >
        {words.length} words · select and press Delete to cut the audio
      </p>

      {words.map((w, i) => {
        const selected = highlight !== null && i >= highlight.from && i < highlight.to;
        return (
          <span
            key={i}
            data-testid="transcript-word"
            data-selected={selected ? "true" : "false"}
            onMouseDown={(e) => {
              if (e.button !== 0) return;
              dragging.current = true;
              setAnchor(i);
              setFocus(i);
            }}
            onMouseEnter={() => {
              if (dragging.current) setFocus(i);
            }}
            onDoubleClick={() => onSeek?.(w.start_sec)}
            title={`${w.start_sec.toFixed(2)}s – ${w.end_sec.toFixed(2)}s`}
            style={{
              cursor: "text",
              padding: "1px 2px",
              borderRadius: 2,
              background: selected ? "var(--accent)" : "transparent",
              color: selected ? "var(--onyx-0, #07080b)" : "inherit",
              // A word the model was unsure of is worth seeing before
              // you cut around it.
              opacity: w.confidence < 0.5 ? 0.55 : 1,
              userSelect: "none",
            }}
          >
            {w.text}{" "}
          </span>
        );
      })}
    </div>
  );
}

function Empty({ children }: { children: React.ReactNode }) {
  return (
    <div
      style={{
        height: "100%",
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        textAlign: "center",
        color: "var(--text-dim)",
        padding: 24,
      }}
    >
      {children}
    </div>
  );
}
