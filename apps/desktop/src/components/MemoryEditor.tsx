/**
 * MemoryEditor — two-textarea pane for the global + project memory
 * files. Powers the `Memory` tab of the Settings panel.
 *
 * Behaviour notes:
 *  - On mount we read both scopes from the backend. The project read
 *    is allowed to fail (no project open); we surface that as a
 *    disabled textarea with an inline note rather than a hard error.
 *  - Each scope has its own save button. Saves are explicit (no
 *    auto-save) because memory edits are deliberate — same model the
 *    user has for `~/.claude/CLAUDE.md`.
 *  - "Save" disables until the value differs from the last persisted
 *    snapshot, so a no-op click is impossible.
 */

import { useEffect, useState } from "react";

import { readMemory, writeMemory, type MemoryScope } from "../lib/tauri-bridge";

interface PaneState {
  /** Last value loaded from / saved to disk. The save button uses
   *  this as the dirty-state baseline. */
  saved: string;
  /** The textarea's current value. */
  draft: string;
  /** Set when the read failed. `"no-project"` is the expected case
   *  for the project pane when no project is open. */
  error: string | null;
  /** True while a save round-trip is in flight. */
  saving: boolean;
  /** Last save outcome — drives the inline status line. */
  status: "idle" | "saved" | "error";
  statusMessage: string;
}

const EMPTY: PaneState = {
  saved: "",
  draft: "",
  error: null,
  saving: false,
  status: "idle",
  statusMessage: "",
};

export function MemoryEditor() {
  const [global, setGlobal] = useState<PaneState>(EMPTY);
  const [project, setProject] = useState<PaneState>(EMPTY);

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        const g = await readMemory("global");
        if (!cancelled)
          setGlobal({ ...EMPTY, saved: g, draft: g });
      } catch (err) {
        if (!cancelled)
          setGlobal({ ...EMPTY, error: String(err) });
      }
      try {
        const p = await readMemory("project");
        if (!cancelled)
          setProject({ ...EMPTY, saved: p, draft: p });
      } catch (err) {
        if (!cancelled)
          setProject({ ...EMPTY, error: String(err) });
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  const handleSave = async (
    scope: MemoryScope,
    pane: PaneState,
    setPane: (p: PaneState) => void,
  ) => {
    setPane({ ...pane, saving: true, status: "idle", statusMessage: "" });
    try {
      await writeMemory(scope, pane.draft);
      setPane({
        ...pane,
        saving: false,
        saved: pane.draft,
        status: "saved",
        statusMessage: "Saved.",
      });
    } catch (err) {
      setPane({
        ...pane,
        saving: false,
        status: "error",
        statusMessage: String(err),
      });
    }
  };

  return (
    <div data-testid="memory-editor" className="space-y-5">
      <Pane
        scope="global"
        label="Global memory"
        helper="Applies to every project. Stored at ~/.edytlab/memory.md."
        pane={global}
        setPane={setGlobal}
        onSave={handleSave}
      />
      <Pane
        scope="project"
        label="Project memory"
        helper="Applies only to the currently open project. Stored at <project>/.edytlab/EDYTLAB.md."
        pane={project}
        setPane={setProject}
        onSave={handleSave}
      />
    </div>
  );
}

interface PaneProps {
  scope: MemoryScope;
  label: string;
  helper: string;
  pane: PaneState;
  setPane: (p: PaneState) => void;
  onSave: (
    scope: MemoryScope,
    pane: PaneState,
    setPane: (p: PaneState) => void,
  ) => Promise<void>;
}

function Pane({ scope, label, helper, pane, setPane, onSave }: PaneProps) {
  // Project-scope read fails with "no project open" when there isn't
  // one; we render the textarea as read-only with an inline hint.
  const projectMissing =
    scope === "project" &&
    pane.error !== null &&
    pane.error.toLowerCase().includes("no project");

  const disabled = projectMissing;
  const dirty = !disabled && pane.draft !== pane.saved;

  return (
    <section data-testid={`memory-pane-${scope}`} className="space-y-1.5">
      <div className="flex items-baseline justify-between">
        <label
          htmlFor={`memory-textarea-${scope}`}
          className="font-mono text-[10px] uppercase tracking-[0.18em] text-[var(--text-faint)]"
        >
          {label}
        </label>
        <span className="text-[10px] text-[var(--text-faint)]">{helper}</span>
      </div>
      <textarea
        id={`memory-textarea-${scope}`}
        data-testid={`memory-textarea-${scope}`}
        value={pane.draft}
        disabled={disabled}
        onChange={(e) =>
          setPane({ ...pane, draft: e.target.value, status: "idle" })
        }
        rows={6}
        placeholder={
          projectMissing
            ? "Open a project to edit project memory."
            : scope === "global"
            ? "Notes the assistant should remember across every project…"
            : "Notes specific to this project…"
        }
        className="
          w-full resize-y
          rounded-md border border-[var(--border-strong)]
          bg-[var(--surface)]
          px-3 py-2 font-mono text-xs text-[var(--text)]
          outline-none
          transition
          placeholder:text-[var(--text-faint)] placeholder:font-sans
          focus:border-[var(--accent)]/55
          focus:shadow-[0_0_0_3px_var(--accent-soft)]
          disabled:cursor-not-allowed disabled:opacity-50
        "
      />
      <div className="flex items-center justify-between gap-2">
        <span
          data-testid={`memory-status-${scope}`}
          className={
            "text-xs " +
            (pane.status === "error"
              ? "text-[var(--danger)]"
              : pane.status === "saved"
              ? "text-[var(--success)]"
              : "text-[var(--text-faint)]")
          }
        >
          {pane.statusMessage ||
            (pane.error && !projectMissing ? pane.error : "")}
        </span>
        <button
          type="button"
          data-testid={`memory-save-${scope}`}
          disabled={!dirty || pane.saving}
          onClick={() => void onSave(scope, pane, setPane)}
          className="
            rounded-md
            bg-[var(--accent)]
            px-3 py-1
            text-xs font-medium text-[var(--bg)]
            shadow-[0_4px_12px_-4px_var(--accent-glow)]
            transition
            hover:bg-[#ffa05f]
            disabled:cursor-not-allowed disabled:bg-[var(--surface-elev-2)] disabled:text-[var(--text-faint)] disabled:shadow-none
          "
        >
          {pane.saving ? "Saving…" : "Save"}
        </button>
      </div>
    </section>
  );
}
