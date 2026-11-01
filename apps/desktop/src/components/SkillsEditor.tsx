/**
 * SkillsEditor — CRUD UI for `~/.edytlab/skills/*.md`.
 *
 * Layout: list on the left, editor on the right. The list calls
 * `listSkills`; selecting a row populates the editor via
 * `readSkill`. The editor has a frontmatter form (name, description,
 * trigger, keywords / pattern, enabled) plus a body textarea. Save
 * goes through `upsertSkill`; Delete confirms then `deleteSkill`s.
 * "New" creates a stub draft that becomes a real skill on first Save.
 * The list / save / delete state lives in `useRecordEditor`.
 */

import {
  deleteSkill,
  listSkills,
  readSkill,
  upsertSkill,
  type SkillContent,
  type SkillSummary,
} from "../lib/tauri-bridge";
import { IDENTIFIER_INPUT } from "../lib/identifierInput";
import { useRecordEditor } from "../lib/useRecordEditor";
import {
  EditorBody,
  EditorFooter,
  BodyRow,
  DescriptionRow,
  EditorLayout,
  NameRow,
  Row,
} from "./RecordEditorParts";

const DRAFT_DEFAULT: SkillContent = {
  name: "",
  description: "",
  trigger: "always",
  keywords: [],
  pattern: "",
  enabled: true,
  body: "",
};

export function SkillsEditor() {
  const ed = useRecordEditor<SkillContent, SkillSummary>({
    fetchList: listSkills,
    read: readSkill,
    upsert: upsertSkill,
    remove: deleteSkill,
    newDraft: () => ({ ...DRAFT_DEFAULT }),
    nameOf: (c) => c.name,
    noun: "skill",
  });
  const { editor, list } = ed;

  return (
    <EditorLayout
      testId="skills-editor"
      asideWidth="w-44"
      title="Skills"
      newTestId="skills-new"
      onNew={ed.startNew}
      listError={ed.listError}
      listErrorTestId="skills-list-error"
      isEmpty={list.length === 0}
      emptyTestId="skills-list-empty"
      emptyText='No skills yet. Click "New" or drop a .md file in ~/.edytlab/skills/.'
      list={
        <ul>
          {list.map((s) => {
            const active =
              editor.kind === "draft" && editor.content.name === s.name;
            return (
              <li key={s.name}>
                <button
                  type="button"
                  data-testid={`skills-row-${s.name}`}
                  onClick={() => void ed.open(s.name)}
                  className={
                    "block w-full truncate px-2 py-1.5 text-left text-xs transition " +
                    (active
                      ? "bg-[var(--accent-soft)] text-[var(--accent-text)]"
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
      }
    >
      <EditorBody
        editor={editor}
        emptyTestId="skills-editor-empty"
        emptyClassName="flex h-full flex-col items-center justify-center gap-1 rounded-md border border-dashed border-[var(--border)] px-4 text-center text-xs text-[var(--text-faint)]"
        empty={
          <>
            <span>Select a skill or click "New" to create one.</span>
            <span
              data-testid="skills-bundled-note"
              className="max-w-sm text-[11px]"
            >
              Skills that come with edytlab are updated when a release changes
              them, unless you have edited them.
            </span>
          </>
        }
        form={(d) => (
          <SkillForm content={d.content} isNew={d.isNew} onChange={ed.change} />
        )}
      />
      <EditorFooter
        prefix="skills"
        editor={editor}
        status={ed.status}
        saving={ed.saving}
        nameOf={(c) => c.name}
        onDelete={() => void ed.remove()}
        onSave={() => void ed.save()}
      />
    </EditorLayout>
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
      <NameRow
        prefix="skills"
        value={content.name}
        locked={!isNew}
        placeholder="mixing-glossary"
        onChange={(name) => onChange({ ...content, name })}
      />
      <DescriptionRow
        prefix="skills"
        value={content.description}
        onChange={(description) => onChange({ ...content, description })}
      />
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
          className="field w-full px-2 py-1 text-xs outline-none transition"
        >
          <option value="always">always</option>
          <option value="keywords">keywords</option>
          <option value="regex">regex</option>
        </select>
      </Row>
      {content.trigger === "keywords" ? (
        <Row label="Keywords (comma-separated)">
          <input
            {...IDENTIFIER_INPUT}
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
            className="field w-full px-2 py-1 font-mono text-xs outline-none transition"
          />
        </Row>
      ) : null}
      {content.trigger === "regex" ? (
        <Row label="Pattern (Rust regex syntax)">
          <input
            {...IDENTIFIER_INPUT}
            type="text"
            data-testid="skills-pattern"
            value={content.pattern}
            onChange={(e) =>
              onChange({ ...content, pattern: e.target.value })
            }
            placeholder="[0-9]+\\s*db"
            className="field w-full px-2 py-1 font-mono text-xs outline-none transition"
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
      <BodyRow
        label="Body"
        prefix="skills"
        value={content.body}
        onChange={(body) => onChange({ ...content, body })}
      />
    </div>
  );
}
