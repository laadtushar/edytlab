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
 * Toggling a tool off does NOT yet filter the request — wiring the
 * disabled-tools list into the agent loop is a follow-up tracked in
 * `docs/specs/agentic-chat-ui.md`. For v1 the toggles persist locally
 * in `localStorage` so the UI is testable end-to-end while the
 * filtering ships next.
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

  // Load capabilities the first time the menu opens. The Rust side
  // resolves synchronously off the in-memory dispatcher so this is
  // cheap, but we still cache to avoid an extra IPC on every reopen.
  useEffect(() => {
    if (!open || caps) return;
    let cancelled = false;
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
  }, [open, caps]);

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
            title="Skills"
            hint="workflow recipes — coming soon"
            items={caps.skills}
            disabled={toggle.disabled}
            onToggle={toggleTool}
            comingSoon
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
            hint="external connectors — coming soon"
            items={caps.mcp_servers}
            disabled={toggle.disabled}
            onToggle={toggleTool}
            comingSoon
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
}

function Group({
  title,
  hint,
  items,
  disabled,
  onToggle,
  comingSoon,
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
          {comingSoon ? "Nothing here yet." : "—"}
        </p>
      ) : (
        <ul className="space-y-0.5">
          {items.map((it) => {
            const off = disabled.has(it.name);
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
                  <input
                    type="checkbox"
                    checked={!off}
                    onChange={() => onToggle(it.name)}
                    aria-label={`Enable ${it.name}`}
                    className="mt-0.5 accent-[var(--accent)]"
                  />
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
