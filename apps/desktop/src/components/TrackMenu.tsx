/**
 * TrackMenu — the per-track dropdown on a lane head (#161).
 *
 * Every action here already existed as a tool. What did not exist was a
 * way to reach one without typing a sentence to the agent, which is a
 * strange thing to have to do to rename a track.
 *
 * Deliberately small: rename, duplicate, remove. Each maps to exactly
 * one existing tool and appends one ordinary session node, so all three
 * undo like any other edit. That is also why removing does not ask "are
 * you sure" — the undo stack is the confirmation, and a modal on top of
 * a reversible action trains people to click through modals.
 */

import { useCallback, useEffect, useRef, useState } from "react";

export interface TrackMenuProps {
  /** Session track index — not the lane's position. */
  trackIndex: number;
  trackName: string;
  onRename: (trackIndex: number, name: string) => void;
  onDuplicate: (trackIndex: number) => void;
  onRemove: (trackIndex: number) => void;
}

export function TrackMenu({
  trackIndex,
  trackName,
  onRename,
  onDuplicate,
  onRemove,
}: TrackMenuProps) {
  const [open, setOpen] = useState(false);
  const [renaming, setRenaming] = useState(false);
  const [draft, setDraft] = useState(trackName);
  const rootRef = useRef<HTMLDivElement>(null);

  const close = useCallback(() => {
    setOpen(false);
    setRenaming(false);
  }, []);

  // Click-away and Escape. A dropdown that only closes by choosing
  // something is a trap, and this one sits over a waveform the user is
  // trying to look at.
  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      if (!rootRef.current?.contains(e.target as Node)) close();
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        close();
      }
    };
    window.addEventListener("mousedown", onDown);
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("mousedown", onDown);
      window.removeEventListener("keydown", onKey);
    };
  }, [open, close]);

  const commitRename = () => {
    const name = draft.trim();
    // An empty name is refused rather than accepted-and-ignored: the
    // field stays open so it is obvious nothing happened.
    if (!name || name === trackName) {
      close();
      return;
    }
    onRename(trackIndex, name);
    close();
  };

  const itemStyle: React.CSSProperties = {
    display: "block",
    width: "100%",
    textAlign: "left",
    background: "transparent",
    border: "none",
    color: "var(--text)",
    fontFamily: "var(--font-mono)",
    fontSize: 11,
    padding: "6px 10px",
    cursor: "pointer",
  };

  return (
    <div ref={rootRef} style={{ position: "relative" }}>
      <button
        type="button"
        data-testid={`track-menu-btn-${trackIndex}`}
        aria-haspopup="menu"
        aria-expanded={open ? "true" : "false"}
        aria-label={`Track actions for ${trackName}`}
        onClick={() => setOpen((v) => !v)}
        style={{
          background: "transparent",
          border: "1px solid var(--border-strong)",
          borderRadius: 4,
          color: "var(--text-dim)",
          fontSize: 11,
          lineHeight: 1,
          padding: "2px 5px",
          cursor: "pointer",
        }}
      >
        ⋯
      </button>

      {open && (
        <div
          role="menu"
          data-testid={`track-menu-${trackIndex}`}
          style={{
            position: "absolute",
            top: "100%",
            left: 0,
            marginTop: 4,
            minWidth: 150,
            background: "var(--surface-elev-2)",
            border: "1px solid var(--border-strong)",
            borderRadius: 6,
            boxShadow: "0 8px 24px rgba(0,0,0,0.45)",
            zIndex: 20,
            padding: 4,
          }}
        >
          {renaming ? (
            <input
              autoFocus
              data-testid={`track-rename-input-${trackIndex}`}
              value={draft}
              onChange={(e) => setDraft(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") commitRename();
                if (e.key === "Escape") close();
                // The lane below listens for single keys (L, space,
                // arrows). Without this, typing a track name would
                // toggle loop and move the playhead.
                e.stopPropagation();
              }}
              onBlur={commitRename}
              style={{
                width: "100%",
                background: "var(--surface)",
                border: "1px solid var(--border-strong)",
                borderRadius: 4,
                color: "var(--text)",
                fontFamily: "var(--font-mono)",
                fontSize: 11,
                padding: "5px 8px",
              }}
            />
          ) : (
            <button
              type="button"
              role="menuitem"
              data-testid={`track-rename-${trackIndex}`}
              style={itemStyle}
              onClick={() => {
                setDraft(trackName);
                setRenaming(true);
              }}
            >
              Rename…
            </button>
          )}

          <button
            type="button"
            role="menuitem"
            data-testid={`track-duplicate-${trackIndex}`}
            style={itemStyle}
            onClick={() => {
              onDuplicate(trackIndex);
              close();
            }}
          >
            Duplicate
          </button>

          <button
            type="button"
            role="menuitem"
            data-testid={`track-remove-${trackIndex}`}
            style={{ ...itemStyle, color: "var(--danger, #ff6b6b)" }}
            onClick={() => {
              onRemove(trackIndex);
              close();
            }}
          >
            Remove
          </button>
        </div>
      )}
    </div>
  );
}
