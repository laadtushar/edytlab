/**
 * AgentProfilesEditor — CRUD UI for `~/.edytlab/agents/*.md` plus the
 * "active profile" selector. Layout mirrors `SkillsEditor`: list on
 * the left, form on the right.
 *
 * Active-profile UX:
 *  - Rows show a star badge when active.
 *  - The form has a "Use this profile" toggle that round-trips through
 *    `setActiveAgentProfile`; toggling off clears the selection.
 */

import { useEffect, useRef, useState } from "react";

import {
  deleteAgentProfile,
  getActiveAgentProfile,
  listAgentProfiles,
  readAgentProfile,
  setActiveAgentProfile,
  upsertAgentProfile,
  type AgentProfileContent,
  type AgentProfileSummary,
} from "../lib/tauri-bridge";

const DRAFT_DEFAULT: AgentProfileContent = {
  name: "",
  description: "",
  model: null,
  tools: null,
  body: "",
};

type EditorState =
  | { kind: "empty" }
  | { kind: "loading"; name: string }
  | {
      kind: "draft";
      content: AgentProfileContent;
      isNew: boolean;
      dirty: boolean;
    };

export function AgentProfilesEditor() {
  const [list, setList] = useState<AgentProfileSummary[]>([]);
  const [active, setActive] = useState<string | null>(null);
  const [listError, setListError] = useState<string | null>(null);
  const [editor, setEditor] = useState<EditorState>({ kind: "empty" });
  const [saving, setSaving] = useState(false);
  const [status, setStatus] = useState<{
    kind: "idle" | "ok" | "err";
    message: string;
  }>({ kind: "idle", message: "" });

  const refresh = async () => {
    try {
      const [l, a] = await Promise.all([
        listAgentProfiles(),
        getActiveAgentProfile(),
      ]);
      setList(l);
      setActive(a);
      setListError(null);
    } catch (err) {
      setListError(String(err));
    }
  };

  useEffect(() => {
    void refresh();
  }, []);

  const open = async (name: string) => {
    setEditor({ kind: "loading", name });
    setStatus({ kind: "idle", message: "" });
    try {
      const content = await readAgentProfile(name);
      setEditor({ kind: "draft", content, isNew: false, dirty: false });
    } catch (err) {
      setStatus({ kind: "err", message: String(err) });
      setEditor({ kind: "empty" });
    }
  };

  const newProfile = () => {
    setEditor({
      kind: "draft",
      content: { ...DRAFT_DEFAULT },
      isNew: true,
      dirty: true,
    });
    setStatus({ kind: "idle", message: "" });
  };

  const handleSave = async () => {
    if (editor.kind !== "draft") return;

    // Creating a profile whose name is already taken used to overwrite it
    // and report "Saved." The backend `upsert` is unconditional by
    // design — it is the same call the edit path uses — so the create
    // path is the only place that can tell the difference between
    // "save my edits" and "make a new one".
    if (editor.isNew && list.some((p) => p.name === editor.content.name)) {
      setStatus({
        kind: "err",
        message: `A profile named "${editor.content.name}" already exists. Choose another name, or open that profile to edit it.`,
      });
      return;
    }

    setSaving(true);
    setStatus({ kind: "idle", message: "" });
    try {
      await upsertAgentProfile(editor.content.name, editor.content);
      await refresh();
      setEditor({
        kind: "draft",
        content: editor.content,
        isNew: false,
        dirty: false,
      });
      setStatus({ kind: "ok", message: "Saved." });
    } catch (err) {
      setStatus({ kind: "err", message: String(err) });
    } finally {
      setSaving(false);
    }
  };

  const handleDelete = async () => {
    if (editor.kind !== "draft" || editor.isNew) return;
    if (
      !window.confirm(
        `Delete profile "${editor.content.name}"? This cannot be undone.`,
      )
    )
      return;
    try {
      await deleteAgentProfile(editor.content.name);
      setEditor({ kind: "empty" });
      await refresh();
      setStatus({ kind: "ok", message: "Deleted." });
    } catch (err) {
      setStatus({ kind: "err", message: String(err) });
    }
  };

  const toggleActive = async (target: string | null) => {
    try {
      await setActiveAgentProfile(target);
      setActive(target);
    } catch (err) {
      setStatus({ kind: "err", message: String(err) });
    }
  };

  return (
    <div data-testid="agent-profiles-editor" className="flex h-[28rem] gap-3">
      <aside className="w-48 flex-shrink-0 overflow-y-auto rounded-md border border-[var(--border-strong)] bg-[var(--surface)]">
        <div className="flex items-center justify-between border-b border-[var(--border)] px-2 py-1.5">
          <span className="font-mono text-[10px] uppercase tracking-[0.18em] text-[var(--text-faint)]">
            Agent profiles
          </span>
          <button
            type="button"
            data-testid="profiles-new"
            onClick={newProfile}
            className="rounded border border-[var(--border-strong)] px-2 py-0.5 text-[10px] uppercase tracking-wider text-[var(--text-dim)] transition hover:border-[var(--accent)]/50 hover:text-[var(--accent)]"
          >
            New
          </button>
        </div>
        {listError ? (
          <p
            data-testid="profiles-list-error"
            className="px-2 py-1 text-[11px] text-[var(--danger)]"
          >
            {listError}
          </p>
        ) : list.length === 0 ? (
          <p
            data-testid="profiles-list-empty"
            className="px-2 py-2 text-[11px] italic text-[var(--text-faint)]"
          >
            No profiles yet. Click "New" or drop a .md file in ~/.edytlab/agents/.
          </p>
        ) : (
          <ul>
            {list.map((p) => {
              const isActiveRow = active === p.name;
              const isEditingRow =
                editor.kind === "draft" && editor.content.name === p.name;
              return (
                <li key={p.name}>
                  <button
                    type="button"
                    data-testid={`profiles-row-${p.name}`}
                    onClick={() => void open(p.name)}
                    className={
                      "block w-full truncate px-2 py-1.5 text-left text-xs transition " +
                      (isEditingRow
                        ? "bg-[var(--accent-soft)] text-[var(--accent)]"
                        : "text-[var(--text-dim)] hover:bg-[var(--surface-elev-2)] hover:text-[var(--text)]")
                    }
                  >
                    <div className="flex items-center justify-between gap-2">
                      <span className="truncate font-mono text-[11px]">
                        {p.name}
                      </span>
                      {isActiveRow ? (
                        <span
                          data-testid={`profiles-active-badge-${p.name}`}
                          className="rounded bg-[var(--accent)]/20 px-1 text-[9px] uppercase tracking-wider text-[var(--accent)]"
                        >
                          active
                        </span>
                      ) : null}
                    </div>
                    <div className="truncate text-[10px] text-[var(--text-faint)]">
                      {p.description || "—"}
                    </div>
                  </button>
                </li>
              );
            })}
          </ul>
        )}
      </aside>

      <section className="flex flex-1 flex-col gap-2">
        {editor.kind === "empty" ? (
          <div
            data-testid="profiles-editor-empty"
            className="flex h-full items-center justify-center rounded-md border border-dashed border-[var(--border)] text-xs text-[var(--text-faint)]"
          >
            Select a profile or click "New" to create one.
          </div>
        ) : editor.kind === "loading" ? (
          <div className="flex h-full items-center justify-center text-xs text-[var(--text-faint)]">
            Loading {editor.name}…
          </div>
        ) : (
          <ProfileForm
            content={editor.content}
            isNew={editor.isNew}
            isActive={!editor.isNew && active === editor.content.name}
            onChange={(c) =>
              setEditor({ ...editor, content: c, dirty: true })
            }
            onToggleActive={(on) =>
              void toggleActive(on ? editor.content.name : null)
            }
          />
        )}

        <div className="flex items-center justify-between gap-2">
          <span
            data-testid="profiles-status"
            className={
              "text-xs " +
              (status.kind === "err"
                ? "text-[var(--danger)]"
                : status.kind === "ok"
                ? "text-[var(--success)]"
                : "text-[var(--text-faint)]")
            }
          >
            {status.message}
          </span>
          <div className="flex items-center gap-2">
            {editor.kind === "draft" && !editor.isNew ? (
              <button
                type="button"
                data-testid="profiles-delete"
                onClick={() => void handleDelete()}
                className="rounded-md border border-[var(--danger)]/40 px-3 py-1 text-xs text-[var(--danger)] transition hover:bg-[var(--danger)]/10"
              >
                Delete
              </button>
            ) : null}
            <button
              type="button"
              data-testid="profiles-save"
              disabled={
                editor.kind !== "draft" ||
                saving ||
                !editor.dirty ||
                editor.content.name.trim() === ""
              }
              onClick={() => void handleSave()}
              className="rounded-md bg-[var(--accent)] px-3 py-1 text-xs font-medium text-[var(--bg)] shadow-[0_4px_12px_-4px_var(--accent-glow)] transition hover:bg-[#ffa05f] disabled:cursor-not-allowed disabled:bg-[var(--surface-elev-2)] disabled:text-[var(--text-faint)] disabled:shadow-none"
            >
              {saving ? "Saving…" : "Save"}
            </button>
          </div>
        </div>
      </section>
    </div>
  );
}

interface ProfileFormProps {
  content: AgentProfileContent;
  isNew: boolean;
  isActive: boolean;
  onChange: (c: AgentProfileContent) => void;
  onToggleActive: (on: boolean) => void;
}

/// A model override is only meaningful with both halves filled in.
///
/// The old rule collapsed to `null` only when *both* boxes were empty, so
/// filling one persisted a half pair. `{provider:"", id:"gpt-5"}` is the
/// natural thing to save when you just want to name a model — and an
/// empty provider resolves to no provider at all, which disables the
/// agent the moment the profile is activated. Treat a half pair as "no
/// override" until the second half arrives.
function coerceModel(
  provider: string,
  id: string,
): { provider: string; id: string } | null {
  if (provider.trim() === "" || id.trim() === "") return null;
  return { provider, id };
}

function ProfileForm({
  content,
  isNew,
  isActive,
  onChange,
  onToggleActive,
}: ProfileFormProps) {
  // The whitelist field holds raw text so a half-typed list survives a
  // keystroke; `content.tools` stays the parsed truth. Re-seeded when the
  // form switches to a different profile, so opening one shows its tools
  // rather than whatever was last typed.
  const [toolsText, setToolsText] = useState((content.tools ?? []).join(", "));

  // The model override's two halves also live in form state. Emitting
  // `null` for a half pair is right for what gets *saved*, but if the
  // fields read back from `content.model` they would lose the first half
  // the moment it collapsed — and the second half could never join it.
  const [modelProvider, setModelProvider] = useState(
    content.model?.provider ?? "",
  );
  const [modelId, setModelId] = useState(content.model?.id ?? "");

  const seededFor = useRef(content.name);
  if (seededFor.current !== content.name) {
    seededFor.current = content.name;
    setToolsText((content.tools ?? []).join(", "));
    setModelProvider(content.model?.provider ?? "");
    setModelId(content.model?.id ?? "");
  }

  return (
    <div className="flex flex-1 flex-col gap-2 overflow-y-auto pr-1">
      <Row label="Name">
        <input
          type="text"
          data-testid="profiles-name"
          value={content.name}
          disabled={!isNew}
          onChange={(e) => onChange({ ...content, name: e.target.value })}
          placeholder="precision-editor"
          className="w-full rounded-md border border-[var(--border-strong)] bg-[var(--surface)] px-2 py-1 font-mono text-xs text-[var(--text)] outline-none transition focus:border-[var(--accent)]/55 focus:shadow-[0_0_0_3px_var(--accent-soft)] disabled:cursor-not-allowed disabled:opacity-60"
        />
      </Row>
      <Row label="Description">
        <input
          type="text"
          data-testid="profiles-description"
          value={content.description}
          onChange={(e) =>
            onChange({ ...content, description: e.target.value })
          }
          className="w-full rounded-md border border-[var(--border-strong)] bg-[var(--surface)] px-2 py-1 text-xs text-[var(--text)] outline-none transition focus:border-[var(--accent)]/55"
        />
      </Row>
      <Row label="Model override (optional)">
        <div className="grid grid-cols-2 gap-1.5">
          <input
            type="text"
            data-testid="profiles-model-provider"
            value={modelProvider}
            placeholder="anthropic"
            onChange={(e) => {
              const provider = e.target.value;
              setModelProvider(provider);
              onChange({ ...content, model: coerceModel(provider, modelId) });
            }}
            className="rounded-md border border-[var(--border-strong)] bg-[var(--surface)] px-2 py-1 font-mono text-xs text-[var(--text)] outline-none transition focus:border-[var(--accent)]/55"
          />
          <input
            type="text"
            data-testid="profiles-model-id"
            value={modelId}
            placeholder="claude-opus-4-7"
            onChange={(e) => {
              const id = e.target.value;
              setModelId(id);
              onChange({ ...content, model: coerceModel(modelProvider, id) });
            }}
            className="rounded-md border border-[var(--border-strong)] bg-[var(--surface)] px-2 py-1 font-mono text-xs text-[var(--text)] outline-none transition focus:border-[var(--accent)]/55"
          />
        </div>
      </Row>
      <Row label="Tools whitelist (comma-separated; leave empty for all)">
        <input
          type="text"
          data-testid="profiles-tools"
          value={toolsText}
          onChange={(e) => {
            // Keep the raw text in local state and parse on the way out.
            //
            // This used to be a controlled input whose value was
            // `tools.join(", ")` while every keystroke re-parsed with
            // `.filter(Boolean)`. That filter deletes the empty segment a
            // freshly typed comma creates, so React restored the joined
            // value and the comma never survived — typing "load, gain"
            // left the field reading "loadgain" and saved a single
            // nonexistent tool. The list was unusable.
            setToolsText(e.target.value);
            const parsed = e.target.value
              .split(",")
              .map((w) => w.trim())
              .filter(Boolean);
            onChange({ ...content, tools: parsed.length === 0 ? null : parsed });
          }}
          placeholder="load, gain, normalize"
          className="w-full rounded-md border border-[var(--border-strong)] bg-[var(--surface)] px-2 py-1 font-mono text-xs text-[var(--text)] outline-none transition focus:border-[var(--accent)]/55"
        />
      </Row>
      <Row label="Active">
        <label className="flex items-center gap-1.5 text-xs text-[var(--text-dim)]">
          <input
            type="checkbox"
            data-testid="profiles-active-toggle"
            checked={isActive}
            disabled={isNew}
            onChange={(e) => onToggleActive(e.target.checked)}
          />
          {isNew ? "Save the profile first" : "Use this profile"}
        </label>
      </Row>
      <Row label="System prompt addition">
        <textarea
          data-testid="profiles-body"
          value={content.body}
          onChange={(e) => onChange({ ...content, body: e.target.value })}
          rows={10}
          placeholder="You are a precision audio editor. Confirm any destructive operation…"
          className="w-full resize-y rounded-md border border-[var(--border-strong)] bg-[var(--surface)] px-2 py-1 font-mono text-xs text-[var(--text)] outline-none transition focus:border-[var(--accent)]/55"
        />
      </Row>
    </div>
  );
}

function Row({
  label,
  children,
}: {
  label: string;
  children: React.ReactNode;
}) {
  return (
    <div className="space-y-1">
      <span className="block font-mono text-[10px] uppercase tracking-[0.18em] text-[var(--text-faint)]">
        {label}
      </span>
      {children}
    </div>
  );
}
