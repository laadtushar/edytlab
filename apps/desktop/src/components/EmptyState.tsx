/**
 * EmptyState — friendly hero shown by the timeline when no audio is
 * loaded yet.
 *
 * Replaces the tiny "Drop an audio file or use the file menu" hint
 * with something the user actually sees on first launch: the logo
 * mark and wordmark, the supported file types, the keyboard shortcut,
 * and a primary "Open Audio…" CTA. Visually anchors the empty state so
 * users don't sit and wonder what to do.
 */

import { LogoMark } from "./LogoMark";
import { RecentProjects } from "./RecentProjects";
import type { RecentProject } from "../lib/tauri-bridge";

interface EmptyStateProps {
  onOpen: () => void;
  /** Open an existing project folder, as opposed to an audio file. */
  onOpenProject?: () => void;
  /** Start a project in an empty folder. */
  onNewProject?: () => void;
  onShowTemplates?: () => void;
  /**
   * Projects this machine has opened. Absent or empty hides the list
   * entirely, which is what a first launch should look like — an empty
   * "Recent" heading is worse than no heading.
   */
  recents?: RecentProject[];
  onOpenRecent?: (path: string) => void;
  onForgetRecent?: (path: string) => void;
}

/**
 * The label for the platform's primary modifier.
 *
 * The menu registers `CmdOrCtrl+O`, which Tauri resolves to Cmd on macOS
 * and Ctrl elsewhere. Hardcoding "Ctrl+O" told every Mac user the wrong
 * key for the app's single most important action — opening a file.
 *
 * `navigator.platform` is deprecated but is what is reliably available
 * inside the webview; `userAgentData` is not implemented in WKWebView,
 * which is the one platform this needs to get right.
 */
function modifierKey(): string {
  if (typeof navigator === "undefined") return "Ctrl";
  const platform = `${navigator.platform ?? ""} ${navigator.userAgent ?? ""}`;
  return /Mac|iPhone|iPad/i.test(platform) ? "\u2318" : "Ctrl";
}

export function EmptyState({
  onOpen,
  onOpenProject,
  onNewProject,
  onShowTemplates,
  recents,
  onOpenRecent,
  onForgetRecent,
}: EmptyStateProps) {
  return (
    <div
      data-testid="empty-state"
      className="
        relative flex h-full w-full flex-col items-center justify-center
        gap-6 px-12 text-center
        app-fade-in
      "
    >
      {/* The logo mark: decorative (it is aria-hidden), and still. */}
      <div className="flex h-12 items-center justify-center">
        <LogoMark size={44} />
      </div>

      <div className="flex flex-col items-center gap-2">
        <h1
          className="
            text-4xl font-semibold tracking-tight text-[var(--text)]
            leading-none
          "
        >
          <span className="gradient-text">edyt</span>lab
        </h1>
        <p className="max-w-md text-sm leading-relaxed text-[var(--text-dim)]">
          Conversational audio editor. Drop a file or pick one to begin —
          the assistant takes over from there.
        </p>
      </div>

      <div className="flex items-center gap-3">
        {onNewProject ? (
          <button
            type="button"
            onClick={onNewProject}
            data-testid="empty-state-new-project-button"
            title="Pick an empty folder — the project is created there"
            className="
              btn-secondary
              inline-flex items-center gap-2
              bg-[var(--surface-elev)]
              px-4 py-2
              text-sm font-medium
              rounded-md
              transition
            "
          >
            New project…
          </button>
        ) : null}
        {onOpenProject ? (
          <button
            type="button"
            onClick={onOpenProject}
            data-testid="empty-state-open-project-button"
            title="Pick a folder that already contains a project"
            className="
              btn-secondary
              inline-flex items-center gap-2
              bg-[var(--surface-elev)]
              px-4 py-2
              text-sm font-medium
              rounded-md
              transition
            "
          >
            Open project…
          </button>
        ) : null}
        {onShowTemplates ? (
          <button
            type="button"
            onClick={onShowTemplates}
            data-testid="empty-state-templates-button"
            className="
              btn-secondary
              inline-flex items-center gap-2
              bg-[var(--surface-elev)]
              px-4 py-2
              text-sm font-medium
              rounded-md
              transition
            "
          >
            Start from template
          </button>
        ) : null}
        <button
          type="button"
          onClick={onOpen}
          data-testid="empty-state-open-button"
          className="
            btn-primary
            inline-flex items-center gap-2
            border border-transparent
            px-4 py-2
            text-sm
            transition
          "
        >
          <svg
            width="14"
            height="14"
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
          Open Audio…
        </button>
        <span className="font-mono text-[11px] text-[var(--text-faint)]">
          or press{" "}
          <kbd
            className="
              inline-flex items-center
              rounded border border-[var(--border-strong)]
              bg-[var(--surface-elev-2)]
              px-1.5 py-0.5
              font-mono text-[10px] text-[var(--text-dim)]
            "
          >
            {modifierKey()}+O
          </kbd>
        </span>
      </div>

      {recents && recents.length > 0 && onOpenRecent && onForgetRecent ? (
        <RecentProjects
          projects={recents}
          onOpen={onOpenRecent}
          onForget={onForgetRecent}
        />
      ) : null}

      <p className="font-mono text-[10px] uppercase tracking-[0.2em] text-[var(--text-faint)]">
        wav · mp3 · flac · ogg
      </p>

      <p className="max-w-sm text-xs text-[var(--text-faint)]">
        Drop an audio file anywhere on this window to load it.
      </p>

      <div className="mt-2 flex flex-wrap items-center justify-center gap-x-5 gap-y-2">
        <Shortcut keys={["Space"]} label="play / pause" />
        <Shortcut keys={["←", "→"]} label="seek 5s" />
        <Shortcut keys={["Home", "End"]} label="jump" />
        <Shortcut keys={["Drag"]} label="select region" />
      </div>

      <div className="card mt-4 max-w-xs px-4 py-3 text-left">
        <p className="mb-2 font-mono text-[10px] uppercase tracking-[0.18em] text-[var(--text-dim)]">
          Then tell the assistant →
        </p>
        <ul className="space-y-1">
          {ASSISTANT_EXAMPLES.map((ex) => (
            <li
              key={ex}
              className="flex items-center gap-2 text-xs text-[var(--text-faint)]"
            >
              <span className="text-[var(--accent)]">›</span>
              <span>{ex}</span>
            </li>
          ))}
        </ul>
      </div>
    </div>
  );
}

interface ShortcutProps {
  keys: string[];
  label: string;
}

function Shortcut({ keys, label }: ShortcutProps) {
  return (
    <span className="flex items-center gap-1.5 font-mono text-[10px] uppercase tracking-wider text-[var(--text-faint)]">
      {keys.map((k, i) => (
        <span key={i} className="flex items-center gap-1.5">
          {i > 0 ? <span className="text-[var(--text-faint)]/60">·</span> : null}
          <kbd className="rounded border border-[var(--border-strong)] bg-[var(--surface-elev-2)] px-1.5 py-0.5 text-[10px] text-[var(--text-dim)]">
            {k}
          </kbd>
        </span>
      ))}
      <span>{label}</span>
    </span>
  );
}

/**
 * Four things worth trying first.
 *
 * "transcribe the audio" used to be the fourth, and it is the one
 * example here that cannot work — the decoder is a stub (#233). The
 * first thing the app suggests should not be the thing that fails, so
 * it is replaced with another edit that does something audible.
 */
export const ASSISTANT_EXAMPLES = [
  "make this 6 dB louder",
  "fade out the last 3 seconds",
  "remove background noise",
  "normalize to -1 dBFS",
];
