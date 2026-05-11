/**
 * SkillsEditor — CRUD UI for `~/.edytlab/skills/*.md`.
 *
 * Layout: list on the left, editor on the right. The list calls
 * `listSkills`; selecting a row populates the editor via
 * `readSkill`. The editor has a frontmatter form (name, description,
 * trigger, keywords / pattern, enabled) plus a body textarea. Save
 * goes through `upsertSkill`; Delete confirms then `deleteSkill`s.
 * "New" creates a stub draft that becomes a real skill on first Save.
 */

import { useEffect, useState } from "react";

import {
  deleteSkill,
  listSkills,
  readSkill,
  upsertSkill,
  type SkillContent,
  type SkillSummary,
} from "../lib/tauri-bridge";

const DRAFT_DEFAULT: SkillContent = {
  name: "",
  description: "",
  trigger: "always",
  keywords: [],
  pattern: "",
  enabled: true,
  body: "",
};

/**
 * `isNew` is `true` for drafts created via the "New" button — the
 * file doesn't exist on disk yet, so Delete is hidden. `dirty` flips
 * to `true` on the first form change after load / save and clears
 * when the save round-trip succeeds. Tracking it explicitly avoids
 * a deep-compare against a baseline copy on every render.
 */
type EditorState =
  | { kind: "empty" }
  | { kind: "loading"; name: string }
  | { kind: "draft"; content: SkillContent; isNew: boolean; dirty: boolean };

export function SkillsEditor() {
  const [list, setList] = useState<SkillSummary[]>([]);
  const [listError, setListError] = useState<string | null>(null);
  const [editor, setEditor] = useState<EditorState>({ kind: "empty" });
  const [saving, setSaving] = useState(false);
  const [status, setStatus] = useState<{
    kind: "idle" | "ok" | "err";
    message: string;
  }>({ kind: "idle", message: "" });

  const refresh = async () => {
    try {
      const s = await listSkills();
      setList(s);
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
      const content = await readSkill(name);
      setEditor({ kind: "draft", content, isNew: false, dirty: false });
    } catch (err) {
      setStatus({ kind: "err", message: String(err) });
      setEditor({ kind: "empty" });
    }
  };

  const newSkill = () => {
    setEditor({
      kind: "draft",
      content: { ...DRAFT_DEFAULT },
      isNew: true,
      // A brand-new draft is dirty by default — Save is enabled the
      // moment the user types a name (the empty-name guard still
      // blocks the actual save).
      dirty: true,
    });
    setStatus({ kind: "idle", message: "" });
  };

  const handleSave = async () => {
    if (editor.kind !== "draft") return;
    setSaving(true);
    setStatus({ kind: "idle", message: "" });
    try {
      await upsertSkill(editor.content.name, editor.content);
      await refresh();
      setEditor({
        kind: "draft",
        content: editor.content,
        // After a successful save the file exists on disk; the draft
        // is no longer "new" and matches what's persisted.
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
      !window.confirm(`Delete skill "${editor.content.name}"? This cannot be undone.`)
    )
      return;
    try {
      await deleteSkill(editor.content.name);
      setEditor({ kind: "empty" });
      await refresh();
      setStatus({ kind: "ok", message: "Deleted." });
    } catch (err) {
      setStatus({ kind: "err", message: String(err) });
    }
  };

  const dirty = editor.kind === "draft" && editor.dirty;

  return (
    <div data-testid="skills-editor" className="flex h-[28rem] gap-3">
      {/* List */}
      <aside className="w-44 flex-shrink-0 overflow-y-auto rounded-md border border-[var(--border-strong)] bg-[var(--surface)]">
        <div className="flex items-center justify-between border-b border-[var(--border)] px-2 py-1.5">
          <span className="font-mono text-[10px] uppercase tracking-[0.18em] text-[var(--text-faint)]">
            Skills
          </span>
          <button
            type="button"
            data-testid="skills-new"
            onClick={newSkill}
            className="rounded border border-[var(--border-strong)] px-2 py-0.5 text-[10px] uppercase tracking-wider text-[var(--text-dim)] transition hover:border-[var(--accent)]/50 hover:text-[var(--accent)]"
          >
            New
          </button>
        </div>
        {listError ? (
          <p
            data-testid="skills-list-error"
            className="px-2 py-1 text-[11px] text-[var(--danger)]"
          >
            {listError}
          </p>
        ) : list.length === 0 ? (
          <p
            data-testid="skills-list-empty"
            className="px-2 py-2 text-[11px] italic text-[var(--text-faint)]"
          >
            No skills yet. Click "New" or drop a .md file in ~/.edytlab/skills/.
          </p>
        ) : (
          <ul>
            {list.map((s) => {
              const active =
                editor.kind === "draft" && editor.content.name === s.name;
              return (
                <li key={s.name}>
                  <button
                    type="button"
                    data-testid={`skills-row-${s.name}`}
                    onClick={() => void open(s.name)}
                    className={
                      "block w-full truncate px-2 py-1.5 text-left text-xs transition " +
                      (active
                        ? "bg-[var(--accent-soft)] text-[var(--accent)]"
                        : "text-[var(--text-dim)] hover:bg-[var(--surface-elev-2)] hover:text-[var(--text)]")
                    }
                  >
                    <div className="truncate font-mono text-[11px]">{s.name}</div>
                    <div className="truncate text-[10px] text-[var(--text-faint)]">
                      {s.trigger}
                      {!s.enabled ? " · off" : ""}
                    </div>
                  </button>
                </li>
              );
            })}
          </ul>
        )}
      </aside>

      {/* Editor */}
      <section className="flex flex-1 flex-col gap-2">
        {editor.kind === "empty" ? (
          <div
            data-testid="skills-editor-empty"
            className="flex h-full items-center justify-center rounded-md border border-dashed border-[var(--border)] text-xs text-[var(--text-faint)]"
          >
            Select a skill or click "New" to create one.
          </div>
        ) : editor.kind === "loading" ? (
          <div className="flex h-full items-center justify-center text-xs text-[var(--text-faint)]">
            Loading {editor.name}…
          </div>
        ) : (
          <SkillForm
            content={editor.content}
            isNew={editor.isNew}
            onChange={(c) =>
              setEditor({ ...editor, content: c, dirty: true })
            }
          />
        )}

        <div className="flex items-center justify-between gap-2">
          <span
            data-testid="skills-status"
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
                data-testid="skills-delete"
                onClick={() => void handleDelete()}
                className="rounded-md border border-[var(--danger)]/40 px-3 py-1 text-xs text-[var(--danger)] transition hover:bg-[var(--danger)]/10"
              >
                Delete
              </button>
            ) : null}
            <button
              type="button"
              data-testid="skills-save"
              disabled={
                editor.kind !== "draft" ||
                saving ||
                !dirty ||
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

interface SkillFormProps {
  content: SkillContent;
  isNew: boolean;
  onChange: (c: SkillContent) => void;
}

function SkillForm({ content, isNew, onChange }: SkillFormProps) {
  return (
    <div className="flex flex-1 flex-col gap-2 overflow-y-auto pr-1">
      <Row label="Name">
        <input
          type="text"
          data-testid="skills-name"
          value={content.name}
          disabled={!isNew}
          onChange={(e) => onChange({ ...content, name: e.target.value })}
          placeholder="mixing-glossary"
          className="w-full rounded-md border border-[var(--border-strong)] bg-[var(--surface)] px-2 py-1 font-mono text-xs text-[var(--text)] outline-none transition focus:border-[var(--accent)]/55 focus:shadow-[0_0_0_3px_var(--accent-soft)] disabled:cursor-not-allowed disabled:opacity-60"
        />
      </Row>
      <Row label="Description">
        <input
          type="text"
          data-testid="skills-description"
          value={content.description}
          onChange={(e) =>
            onChange({ ...content, description: e.target.value })
          }
          className="w-full rounded-md border border-[var(--border-strong)] bg-[var(--surface)] px-2 py-1 text-xs text-[var(--text)] outline-none transition focus:border-[var(--accent)]/55 focus:shadow-[0_0_0_3px_var(--accent-soft)]"
        />
      </Row>
      <Row label="Trigger">
        <select
          data-testid="skills-trigger"
          value={content.trigger}
          onChange={(e) =>
            onChange({
              ...content,
              trigger: e.target.value as SkillContent["trigger"],
            })
          }
          className="w-full rounded-md border border-[var(--border-strong)] bg-[var(--surface)] px-2 py-1 text-xs text-[var(--text)] outline-none transition focus:border-[var(--accent)]/55"
        >
          <option value="always">always</option>
          <option value="keywords">keywords</option>
          <option value="regex">regex</option>
        </select>
      </Row>
      {content.trigger === "keywords" ? (
        <Row label="Keywords (comma-separated)">
          <input
            type="text"
            data-testid="skills-keywords"
            value={content.keywords.join(", ")}
            onChange={(e) =>
              onChange({
                ...content,
                keywords: e.target.value
                  .split(",")
                  .map((w) => w.trim())
                  .filter(Boolean),
              })
            }
            placeholder="mix, sidechain, bus"
            className="w-full rounded-md border border-[var(--border-strong)] bg-[var(--surface)] px-2 py-1 font-mono text-xs text-[var(--text)] outline-none transition focus:border-[var(--accent)]/55"
          />
        </Row>
      ) : null}
      {content.trigger === "regex" ? (
        <Row label="Pattern (Rust regex syntax)">
          <input
            type="text"
            data-testid="skills-pattern"
            value={content.pattern}
            onChange={(e) =>
              onChange({ ...content, pattern: e.target.value })
            }
            placeholder="[0-9]+\\s*db"
            className="w-full rounded-md border border-[var(--border-strong)] bg-[var(--surface)] px-2 py-1 font-mono text-xs text-[var(--text)] outline-none transition focus:border-[var(--accent)]/55"
          />
        </Row>
      ) : null}
      <Row label="Enabled">
        <label className="flex items-center gap-1.5 text-xs text-[var(--text-dim)]">
          <input
            type="checkbox"
            data-testid="skills-enabled"
            checked={content.enabled}
            onChange={(e) =>
              onChange({ ...content, enabled: e.target.checked })
            }
          />
          on
        </label>
      </Row>
      <Row label="Body">
        <textarea
          data-testid="skills-body"
          value={content.body}
          onChange={(e) => onChange({ ...content, body: e.target.value })}
          rows={10}
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
