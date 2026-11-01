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

import { useRef, useState } from "react";

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

const DRAFT_DEFAULT: AgentProfileContent = {
  name: "",
  description: "",
  model: null,
  tools: null,
  body: "",
};

export function AgentProfilesEditor() {
  const [active, setActive] = useState<string | null>(null);
  const ed = useRecordEditor<AgentProfileContent, AgentProfileSummary>({
    fetchList: async () => {
      const [l, a] = await Promise.all([
        listAgentProfiles(),
        getActiveAgentProfile(),
      ]);
      setActive(a);
      return l;
    },
    read: readAgentProfile,
    upsert: upsertAgentProfile,
    remove: deleteAgentProfile,
    newDraft: () => ({ ...DRAFT_DEFAULT }),
    nameOf: (c) => c.name,
    noun: "profile",
    // Creating a profile whose name is already taken used to overwrite it
    // and report "Saved." The backend `upsert` is unconditional by
    // design — it is the same call the edit path uses — so the create
    // path is the only place that can tell the difference between
    // "save my edits" and "make a new one".
    validateSave: (draft, isNew, list) =>
      isNew && list.some((p) => p.name === draft.name)
        ? `A profile named "${draft.name}" already exists. Choose another name, or open that profile to edit it.`
        : null,
  });
  const { editor, list } = ed;

  const toggleActive = async (target: string | null) => {
    try {
      await setActiveAgentProfile(target);
      setActive(target);
    } catch (err) {
      ed.setStatus({ kind: "err", message: String(err) });
    }
  };

  return (
    <EditorLayout
      testId="agent-profiles-editor"
      asideWidth="w-48"
      title="Agent profiles"
      newTestId="profiles-new"
      onNew={ed.startNew}
      listError={ed.listError}
      listErrorTestId="profiles-list-error"
      isEmpty={list.length === 0}
      emptyTestId="profiles-list-empty"
      emptyText='No profiles yet. Click "New" or drop a .md file in ~/.edytlab/agents/.'
      list={
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
                  onClick={() => void ed.open(p.name)}
                  className={
                    "block w-full truncate px-2 py-1.5 text-left text-xs transition " +
                    (isEditingRow
                      ? "bg-[var(--accent-soft)] text-[var(--accent-text)]"
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
                        className="rounded bg-[var(--accent)]/20 px-1 text-[9px] uppercase tracking-wider text-[var(--accent-text)]"
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
      }
    >
      <EditorBody
        editor={editor}
        emptyTestId="profiles-editor-empty"
        emptyClassName="flex h-full items-center justify-center rounded-md border border-dashed border-[var(--border)] text-xs text-[var(--text-faint)]"
        empty='Select a profile or click "New" to create one.'
        form={(d) => (
          <ProfileForm
            content={d.content}
            isNew={d.isNew}
            isActive={!d.isNew && active === d.content.name}
            onChange={ed.change}
            onToggleActive={(on) =>
              void toggleActive(on ? d.content.name : null)
            }
          />
        )}
      />
      <EditorFooter
        prefix="profiles"
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
      <NameRow
        prefix="profiles"
        value={content.name}
        locked={!isNew}
        placeholder="precision-editor"
        onChange={(name) => onChange({ ...content, name })}
      />
      <DescriptionRow
        prefix="profiles"
        value={content.description}
        onChange={(description) => onChange({ ...content, description })}
      />
      <Row label="Model override (optional)">
        <div className="grid grid-cols-2 gap-1.5">
          <input
            {...IDENTIFIER_INPUT}
            type="text"
            data-testid="profiles-model-provider"
            value={modelProvider}
            placeholder="anthropic"
            onChange={(e) => {
              const provider = e.target.value;
              setModelProvider(provider);
              onChange({ ...content, model: coerceModel(provider, modelId) });
            }}
            className="field px-2 py-1 font-mono text-xs outline-none transition"
          />
          <input
            {...IDENTIFIER_INPUT}
            type="text"
            data-testid="profiles-model-id"
            value={modelId}
            placeholder="claude-opus-4-7"
            onChange={(e) => {
              const id = e.target.value;
              setModelId(id);
              onChange({ ...content, model: coerceModel(modelProvider, id) });
            }}
            className="field px-2 py-1 font-mono text-xs outline-none transition"
          />
        </div>
      </Row>
      <Row label="Tools whitelist (comma-separated; leave empty for all)">
        <input
          {...IDENTIFIER_INPUT}
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
          className="field w-full px-2 py-1 font-mono text-xs outline-none transition"
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
      <BodyRow
        label="System prompt addition"
        prefix="profiles"
        value={content.body}
        placeholder="You are a precision audio editor. Confirm any destructive operation…"
        onChange={(body) => onChange({ ...content, body })}
      />
    </div>
  );
}
