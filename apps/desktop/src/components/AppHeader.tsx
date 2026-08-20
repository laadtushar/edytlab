/**
 * AppHeader — wordmark, view tabs and the primary actions.
 *
 * Lifted out of `App.tsx` so it can be rendered on its own in a test.
 * It was inline, which meant the only way to exercise a header button
 * was to mount the entire application and every Tauri binding with it.
 *
 * Purely presentational: every action is a prop, and the header decides
 * nothing except whether a control is worth drawing.
 */

import type { LeftView } from "../lib/views";

export interface AppHeaderProps {
  leftView: LeftView;
  onSelectView: (v: LeftView) => void;
  onOpen: () => void;
  onSettings: () => void;
  isRecording: boolean;
  onRecord: () => void;
  /**
   * Copy the project elsewhere and carry on there. Omitted — or with no
   * project open — the button is not drawn: there is nothing to copy.
   */
  onSaveAs?: () => void;
  hasProject?: boolean;
  /**
   * Start a project in an empty folder, and open an existing one.
   *
   * These are in the header rather than only on the empty state
   * because the empty state stops existing the moment audio loads —
   * so mid-session there was no way to reach either, and no way at all
   * to leave the current project for another one.
   */
  onNewProject?: () => void;
  onOpenProject?: () => void;
}

export function AppHeader({
  leftView,
  onSelectView,
  onOpen,
  onSettings,
  isRecording,
  onRecord,
  onSaveAs,
  hasProject,
  onNewProject,
  onOpenProject,
}: AppHeaderProps) {
  return (
    <header
      data-testid="left-pane-tabs"
      className="
        relative z-10 flex shrink-0 items-center gap-4
        border-b border-[var(--border)]
        bg-[var(--surface-elev)]
        px-4 py-2.5
      "
    >
      <Wordmark />
      <div
        className="
          ml-3 flex items-center gap-1
          rounded-md border border-[var(--border)]
          bg-[var(--surface)]
          p-0.5
        "
      >
        <TabButton
          label="Timeline"
          testId="tab-timeline"
          active={leftView === "timeline"}
          onClick={() => onSelectView("timeline")}
        />
        <TabButton
          label="Transcript"
          testId="tab-transcript"
          active={leftView === "transcript"}
          onClick={() => onSelectView("transcript")}
        />
        <TabButton
          label="Graph"
          testId="tab-graph"
          active={leftView === "graph"}
          onClick={() => onSelectView("graph")}
        />
      </div>

      <div className="ml-auto flex items-center gap-2">
        {onNewProject ? (
          <button
            type="button"
            data-testid="new-project-button"
            onClick={onNewProject}
            title="Start a new project in an empty folder"
            className="
              inline-flex items-center gap-2
              rounded-md border border-[var(--border-strong)]
              bg-[var(--surface)]
              px-3 py-1.5
              font-mono text-[11px] uppercase tracking-wider text-[var(--text-dim)]
              transition
              hover:border-[var(--accent)]/50 hover:bg-[var(--accent-soft)] hover:text-[var(--accent)]
            "
          >
            New project…
          </button>
        ) : null}
        {onOpenProject ? (
          <button
            type="button"
            data-testid="open-project-button"
            onClick={onOpenProject}
            title="Open an existing project folder"
            className="
              inline-flex items-center gap-2
              rounded-md border border-[var(--border-strong)]
              bg-[var(--surface)]
              px-3 py-1.5
              font-mono text-[11px] uppercase tracking-wider text-[var(--text-dim)]
              transition
              hover:border-[var(--accent)]/50 hover:bg-[var(--accent-soft)] hover:text-[var(--accent)]
            "
          >
            Open project…
          </button>
        ) : null}
        {onSaveAs && hasProject ? (
          <button
            type="button"
            data-testid="save-as-button"
            onClick={onSaveAs}
            title="Copy this project to a new folder and continue there"
            className="
              inline-flex items-center gap-2
              rounded-md border border-[var(--border-strong)]
              bg-[var(--surface)]
              px-3 py-1.5
              font-mono text-[11px] uppercase tracking-wider text-[var(--text-dim)]
              transition
              hover:border-[var(--accent)]/50 hover:bg-[var(--accent-soft)] hover:text-[var(--accent)]
            "
          >
            Save as…
          </button>
        ) : null}
        <button
          type="button"
          data-testid="open-audio-button"
          onClick={onOpen}
          className="
            inline-flex items-center gap-2
            rounded-md border border-[var(--border-strong)]
            bg-[var(--surface)]
            px-3 py-1.5
            font-mono text-[11px] uppercase tracking-wider text-[var(--text-dim)]
            transition
            hover:border-[var(--accent)]/50 hover:bg-[var(--accent-soft)] hover:text-[var(--accent)]
          "
        >
          <svg
            width="12"
            height="12"
            viewBox="0 0 14 14"
            fill="none"
            stroke="currentColor"
            strokeWidth="1.7"
            strokeLinecap="round"
            strokeLinejoin="round"
            aria-hidden="true"
          >
            <path d="M2 3.2C2 2.54 2.54 2 3.2 2h2.6L7 3.5h3.8c.66 0 1.2.54 1.2 1.2v6.1c0 .66-.54 1.2-1.2 1.2H3.2C2.54 12 2 11.46 2 10.8V3.2Z" />
          </svg>
          Open Audio
        </button>

        <button
          type="button"
          data-testid="record-btn"
          onClick={onRecord}
          className={`px-3 py-1 text-sm rounded font-medium ${
            isRecording
              ? "bg-red-600 text-white animate-pulse"
              : "bg-neutral-700 text-neutral-300 hover:bg-neutral-600"
          }`}
        >
          {isRecording ? "⏹ Stop" : "⏺ Record"}
        </button>

        <button
          type="button"
          onClick={onSettings}
          data-testid="open-settings-button"
          aria-label="Open settings"
          className="
            inline-flex h-8 w-8 items-center justify-center
            rounded-md border border-[var(--border-strong)]
            bg-[var(--surface)]
            text-[var(--text-dim)]
            transition
            hover:border-[var(--accent)]/50 hover:bg-[var(--surface-elev-2)] hover:text-[var(--text)]
          "
        >
          <svg
            width="14"
            height="14"
            viewBox="0 0 14 14"
            fill="none"
            stroke="currentColor"
            strokeWidth="1.5"
            strokeLinecap="round"
            strokeLinejoin="round"
            aria-hidden="true"
          >
            <circle cx="7" cy="7" r="2.2" />
            <path d="M7 1.5v2M7 10.5v2M1.5 7h2M10.5 7h2M3 3l1.4 1.4M9.6 9.6L11 11M3 11l1.4-1.4M9.6 4.4L11 3" />
          </svg>
        </button>
      </div>
    </header>
  );
}

function Wordmark() {
  return (
    <div
      className="
        flex items-baseline gap-1
        text-[15px] font-medium leading-none
        text-[var(--text)]
      "
    >
      <span className="font-[family-name:var(--font-serif)] italic text-[var(--accent)] text-[18px]">
        edyt
      </span>
      <span>lab</span>
      <span
        className="
          ml-2 rounded
          bg-[var(--surface-elev-2)]
          px-1.5 py-0.5
          font-mono text-[9px] uppercase tracking-[0.18em] text-[var(--text-faint)]
        "
      >
        studio
      </span>
    </div>
  );
}

interface TabButtonProps {
  label: string;
  testId: string;
  active: boolean;
  onClick: () => void;
}

function TabButton({ label, testId, active, onClick }: TabButtonProps) {
  return (
    <button
      type="button"
      data-testid={testId}
      data-active={active ? "true" : "false"}
      onClick={onClick}
      aria-pressed={active}
      className={
        "rounded px-3 py-1 text-xs font-medium transition " +
        (active
          ? "bg-[var(--surface-elev-2)] text-[var(--text)] shadow-[inset_0_0_0_1px_rgba(255,255,255,0.04)]"
          : "text-[var(--text-faint)] hover:bg-[var(--surface-elev-2)]/60 hover:text-[var(--text-dim)]")
      }
    >
      {label}
    </button>
  );
}

