/**
 * RecentProjects — the list of projects this machine has opened (#156).
 *
 * Launching used to show an empty timeline: no way back to yesterday's
 * work except remembering where you put it and opening the folder
 * again. The backend has recorded recents since the project object
 * landed; this is the part that shows them.
 *
 * Deliberately not a modal or a separate screen. It sits under the
 * empty state's call to action, so "open something new" and "carry on
 * with something" are the same view — which is what they are.
 */

import type { RecentProject } from "../lib/tauri-bridge";

export interface RecentProjectsProps {
  projects: RecentProject[];
  onOpen: (path: string) => void;
  /** Drop a row without touching the project it points at. */
  onForget: (path: string) => void;
}

/**
 * "3 days ago" rather than a timestamp.
 *
 * A recents list is scanned, not read: the useful question is "is this
 * the one I had open yesterday", and an ISO string answers it slowly.
 * Anything older than a week gets the date, because "23 days ago" is
 * not a thing anyone counts.
 */
export function relativeTime(iso: string | null | undefined, now = Date.now()): string {
  if (!iso) return "";
  const then = Date.parse(iso);
  if (Number.isNaN(then)) return "";
  const seconds = Math.max(0, Math.round((now - then) / 1000));
  if (seconds < 60) return "just now";
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) return `${minutes} minute${minutes === 1 ? "" : "s"} ago`;
  const hours = Math.round(minutes / 60);
  if (hours < 24) return `${hours} hour${hours === 1 ? "" : "s"} ago`;
  const days = Math.round(hours / 24);
  if (days <= 7) return `${days} day${days === 1 ? "" : "s"} ago`;
  return new Date(then).toLocaleDateString();
}

/**
 * The folder a project lives in, shortened from the left.
 *
 * Two projects can share a name — "episode-1" under two clients — so
 * the path has to be visible. The interesting half of a long path is
 * the end, so that is the half kept.
 */
export function shortPath(path: string, max = 44): string {
  if (path.length <= max) return path;
  return `…${path.slice(-(max - 1))}`;
}

export function RecentProjects({
  projects,
  onOpen,
  onForget,
}: RecentProjectsProps) {
  if (projects.length === 0) return null;

  return (
    <div
      data-testid="recent-projects"
      className="mt-2 flex w-full max-w-lg flex-col gap-1"
    >
      <span className="px-1 pb-1 font-mono text-[10px] uppercase tracking-[0.18em] text-[var(--text-faint)]">
        Recent
      </span>
      {projects.map((p) => (
        <div
          key={p.path}
          data-testid="recent-project-row"
          className="
            group flex items-center gap-3 rounded-md
            border border-transparent px-3 py-2 text-left
            transition hover:border-[var(--border-strong)] hover:bg-[var(--surface-elev)]
          "
        >
          <button
            type="button"
            data-testid={`recent-open-${p.path}`}
            onClick={() => onOpen(p.path)}
            title={p.path}
            className="flex min-w-0 flex-1 flex-col items-start text-left"
          >
            <span className="w-full truncate text-sm text-[var(--text)]">
              {p.name}
            </span>
            <span className="w-full truncate font-mono text-[10px] text-[var(--text-faint)]">
              {shortPath(p.path)}
            </span>
          </button>
          <span className="shrink-0 font-mono text-[10px] text-[var(--text-faint)]">
            {relativeTime(p.last_opened_at)}
          </span>
          {/*
            Removing a row is not removing a project, so it is quiet:
            visible on hover, and labelled for anyone who cannot hover.
          */}
          <button
            type="button"
            data-testid={`recent-forget-${p.path}`}
            aria-label={`Remove ${p.name} from recent projects`}
            onClick={() => onForget(p.path)}
            className="
              shrink-0 rounded px-1.5 py-0.5 text-[11px] leading-none
              text-[var(--text-faint)] opacity-0 transition
              hover:text-[var(--text-dim)] focus:opacity-100 group-hover:opacity-100
            "
          >
            ✕
          </button>
        </div>
      ))}
    </div>
  );
}
