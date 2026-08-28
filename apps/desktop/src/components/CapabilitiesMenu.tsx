/**
 * CapabilitiesMenu — popover triggered by the `+` in the composer.
 *
 * Lists every capability the agent can currently invoke. The
 * `tools` section is populated from the Rust dispatcher via
 * `listCapabilities` (Tauri command). `skills`, `agents`, and
 * `mcp_servers` are placeholders in the schema today; the menu still
 * renders the group with a "coming soon" hint so the surface is
 * obvious to the user and stable for the implementation that lands
 * later.
 *
 * Toggling a tool off removes it from the schema list sent to the model
 * *and* refuses it at dispatch (#238). The second half is what makes
 * the checkbox a control rather than a hint: the schema list only tells
 * a well-behaved model what to ask for, and a disabled tool was still
 * reachable both by a model that named it anyway and — deterministically
 * — through meta-tools like `batch_apply`, which used to build their own
 * dispatcher that had never seen the whitelist.
 *
 * The toggles persist in `localStorage`, so they are per-machine and
 * per-browser-profile rather than part of the session.
 */

import { useEffect, useRef, useState } from "react";

import type {
  Capabilities,
  CapabilityDescriptor,
} from "../lib/tauri-bridge";
import { listCapabilities } from "../lib/tauri-bridge";

const LS_KEY = "edytlab.capabilities.disabled";

interface ToggleState {
  disabled: Set<string>;
}

function loadToggleState(): ToggleState {
  try {
    const raw = localStorage.getItem(LS_KEY);
    if (!raw) return { disabled: new Set() };
    const arr = JSON.parse(raw) as unknown;
    if (Array.isArray(arr)) {
      return { disabled: new Set(arr.filter((x): x is string => typeof x === "string")) };
    }
  } catch {
    // Fall through to default — a corrupt key is harmless.
  }
  return { disabled: new Set() };
}

function persistToggleState(state: ToggleState) {
  try {
    localStorage.setItem(LS_KEY, JSON.stringify(Array.from(state.disabled)));
  } catch {
    // localStorage failures (private mode, quota, etc.) are non-fatal.
  }
}

export interface CapabilitiesMenuProps {
  /**
   * Whether the popover is open. The composer owns this state so the
   * `+` button can be the single trigger.
   */
  open: boolean;
  /** Called when the user clicks outside / presses Escape. */
  onClose: () => void;
}

export function CapabilitiesMenu({ open, onClose }: CapabilitiesMenuProps) {
  const [caps, setCaps] = useState<Capabilities | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [toggle, setToggle] = useState<ToggleState>(() => loadToggleState());
  const popRef = useRef<HTMLDivElement>(null);

  // Reload every time the menu opens.
  //
  // This used to fetch once and keep the result for the app's lifetime,
  // which defeated the backend: `list_capabilities` deliberately calls
  // `reload_skills_from_disk()` and reads MCP tools live, precisely so
  // this surface stays current. A user who followed the empty state's own
  // instruction — "Drop a .md file under ~/.edytlab/skills/" — reopened
  // the menu and still read "No skills yet." until they restarted the
  // app, while Settings → Skills showed the same skill perfectly well.
  //
  // The call resolves off in-memory state, so re-running it per open is
  // cheap and always right.
  useEffect(() => {
    if (!open) return;
    let cancelled = false;
    // Clear the previous error, or one transient IPC failure pins the
    // error banner forever — it is checked before `caps`, so the menu
    // never recovers even though the retry would have succeeded.
    setError(null);
    listCapabilities()
      .then((c) => {
        if (!cancelled) setCaps(c);
      })
      .catch((e: unknown) => {
        if (!cancelled) setError(String(e));
      });
    return () => {
      cancelled = true;
    };
  }, [open]);

  // Close on outside click + Escape.
  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      const el = popRef.current;
      if (el && !el.contains(e.target as Node)) {
        onClose();
      }
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    document.addEventListener("mousedown", onDown);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", onDown);
      document.removeEventListener("keydown", onKey);
    };
  }, [open, onClose]);

  if (!open) return null;

  const toggleTool = (name: string) => {
    setToggle((prev) => {
      const next = new Set(prev.disabled);
      if (next.has(name)) next.delete(name);
      else next.add(name);
      const out = { disabled: next };
      persistToggleState(out);
      return out;
    });
  };

  return (
    <div
      ref={popRef}
      data-testid="capabilities-menu"
      role="dialog"
      aria-label="Agent capabilities"
      className="
        absolute bottom-full left-2 mb-2 z-20
        w-72 max-h-80 overflow-y-auto
        rounded-lg border border-[var(--border-strong)]
        bg-[var(--surface-elev)]
        p-2 shadow-[0_10px_30px_-10px_rgba(0,0,0,0.6)]
      "
    >
      {error ? (
        <p className="px-2 py-1 text-xs text-[var(--danger)]">
          Failed to load capabilities: {error}
        </p>
      ) : !caps ? (
        <p className="px-2 py-1 text-xs text-[var(--text-faint)]">
          Loading capabilities…
        </p>
      ) : (
        <>
          <Group
            title="Tools"
            hint="built-in audio engine ops"
            items={caps.tools}
            disabled={toggle.disabled}
            onToggle={toggleTool}
          />
          <Group
            readOnly
            title="Skills"
            hint="markdown rules from ~/.edytlab/skills/"
            items={caps.skills}
            disabled={toggle.disabled}
            onToggle={toggleTool}
            emptyText="No skills yet. Drop a .md file under ~/.edytlab/skills/."
          />
          <Group
            title="Agents"
            hint="specialized sub-agents — coming soon"
            items={caps.agents}
            disabled={toggle.disabled}
            onToggle={toggleTool}
            comingSoon
          />
          <Group
            title="MCP servers"
            hint="tools from connected servers"
            items={caps.mcp_servers}
            disabled={toggle.disabled}
            onToggle={toggleTool}
            emptyText="No MCP servers connected. Add one in Settings → MCP Servers."
          />
        </>
      )}
    </div>
  );
}

interface GroupProps {
  title: string;
  hint: string;
  items: CapabilityDescriptor[];
  disabled: Set<string>;
  onToggle: (name: string) => void;
  comingSoon?: boolean;
  /**
   * Render rows without a checkbox.
   *
   * For groups whose entries the disabled-list cannot filter. The tool
   * blacklist is matched against dispatcher tool names only, so a skill
   * checkbox persisted a preference nothing acted on — the row appeared
   * switched off while the skill kept being injected into the system
   * prompt. Showing no control is honest; showing a dead one is not.
   */
  readOnly?: boolean;
  emptyText?: string;
}

function Group({
  title,
  hint,
  items,
  disabled,
  onToggle,
  comingSoon,
  readOnly,
  emptyText,
}: GroupProps) {
  return (
    <div className="mb-1.5 last:mb-0">
      <div className="flex items-baseline justify-between px-2 pt-1.5 pb-0.5">
        <span className="font-mono text-[10px] uppercase tracking-[0.18em] text-[var(--text-dim)]">
          {title}
        </span>
        <span className="font-mono text-[9px] uppercase tracking-wider text-[var(--text-faint)]">
          {hint}
        </span>
      </div>
      {items.length === 0 ? (
        <p
          data-testid={`group-${title}-empty`}
          className="px-2 py-1 text-[11px] italic text-[var(--text-faint)]"
        >
          {emptyText ?? (comingSoon ? "Nothing here yet." : "—")}
        </p>
      ) : (
        <ul className="space-y-0.5">
          {items.map((it) => {
            // Keyed on `id`, not `name`: for MCP tools those differ, and
            // persisting the display name meant the backend blacklist —
            // which matches dispatcher wire names — never saw a match.
            const off = disabled.has(it.id);
            return (
              <li key={it.name}>
                <label
                  data-testid={`capability-row-${it.name}`}
                  className="
                    flex cursor-pointer items-start gap-2
                    rounded px-2 py-1
                    text-xs text-[var(--text)]
                    hover:bg-[var(--surface-elev-2)]
                  "
                >
                  {readOnly ? null : (
                  <input
                    type="checkbox"
                    checked={!off}
                    onChange={() => onToggle(it.id)}
                    aria-label={`Enable ${it.name}`}
                    className="mt-0.5 accent-[var(--accent)]"
                  />
                  )}
                  <span className="flex flex-col">
                    <span className="font-mono text-[11px] text-[var(--text)]">
                      {it.name}
                    </span>
                    {it.description ? (
                      <span className="text-[10px] leading-snug text-[var(--text-dim)]">
                        {firstSentence(it.description)}
                      </span>
                    ) : null}
                  </span>
                </label>
              </li>
            );
          })}
        </ul>
      )}
    </div>
  );
}

/** First sentence (`.`-terminated) of a long description. */
function firstSentence(s: string): string {
  const dot = s.indexOf(".");
  if (dot < 0 || dot > 140) return s.length > 140 ? s.slice(0, 140) + "…" : s;
  return s.slice(0, dot + 1);
}
