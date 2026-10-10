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
import { LogoMark } from "./LogoMark";
import { TimerRecord } from "./TimerRecord";

export interface AppHeaderProps {
  leftView: LeftView;
  onSelectView: (v: LeftView) => void;
  onOpen: () => void;
  onSettings: () => void;
  isRecording: boolean;
  onRecord: () => void;
  /**
   * Arm an unattended take (#225 §4). Omitting it hides the control,
   * which is what every existing caller and test gets.
   */
  onTimerRecord?: (schedule: {
    startAfterSec?: number;
    durationSec?: number;
  }) => void;
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
  onTimerRecord,
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
              btn-tool inline-flex items-center gap-2
              rounded-md px-3 py-1.5
              font-mono text-[11px] uppercase tracking-wider
              transition
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
              btn-tool inline-flex items-center gap-2
              rounded-md px-3 py-1.5
              font-mono text-[11px] uppercase tracking-wider
              transition
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
              btn-tool inline-flex items-center gap-2
              rounded-md px-3 py-1.5
              font-mono text-[11px] uppercase tracking-wider
              transition
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
            btn-tool inline-flex items-center gap-2
            rounded-md px-3 py-1.5
            font-mono text-[11px] uppercase tracking-wider
            transition
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
          // 11px / 3px, not 12 / 4: the 1px border is new, and the
          // button keeps the box it had.
          className={`rounded-md px-[11px] py-[3px] text-sm font-medium ${
            isRecording
              ? "border border-[var(--danger)] bg-[var(--danger)] text-[var(--accent-fg)] animate-pulse"
              : "btn-secondary"
          }`}
        >
          {isRecording ? (
            "⏹ Stop"
          ) : (
            <>
              <span className="text-[var(--danger)]">⏺</span> Record
            </>
          )}
        </button>

        {onTimerRecord ? (
          <TimerRecord busy={isRecording} onArm={onTimerRecord} />
        ) : null}

        <button
          type="button"
          onClick={onSettings}
          data-testid="open-settings-button"
          aria-label="Open settings"
          className="
            btn-tool inline-flex h-8 w-8 items-center justify-center
            rounded-md
            transition
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
      {/* The site header's accent hairline, over the bottom border. */}
      <span aria-hidden="true" className="rule-accent -bottom-px" />
    </header>
  );
}

/**
 * The site's header lockup: the mark, then "edytlab" in semibold sans.
 * The mark is decorative (`aria-hidden`), so the name is still just the
 * text.
 */
function Wordmark() {
  return (
    <div
      className="
        flex items-center gap-2
        text-[15px] leading-none
        text-[var(--text)]
      "
    >
      <LogoMark size={20} />
      <span className="font-semibold tracking-tight">edytlab</span>
      <span
        className="
          pill ml-1
          px-1.5 py-0.5
          font-mono text-[9px] uppercase tracking-[0.18em] text-[var(--text-dim)]
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
        // The accent bar is `.tab-indicator`'s pseudo-element: it takes no
        // room, so the tabs are the width they were. `overflow-hidden`
        // keeps the bar inside the tab's rounded corners.
        "tab-indicator overflow-hidden rounded px-3 py-1 text-xs font-medium transition " +
        (active
          ? "bg-[var(--surface-elev-2)] text-[var(--text)]"
          : "text-[var(--text-dim)] hover:bg-[var(--surface-elev-2)]/60 hover:text-[var(--text)]")
      }
    >
      {label}
    </button>
  );
}

