/**
 * useRecordEditor — the list / open / new / save / delete state machine
 * shared by the Settings editors (`SkillsEditor`, `AgentProfilesEditor`,
 * `McpServersEditor`). Each editor supplies its bridge calls and labels;
 * this owns the state and the status messages, which were identical.
 */

import { useEffect, useRef, useState } from "react";

import { confirmAction } from "./confirmAction";

/**
 * `isNew` is `true` for drafts created via "New" — the file doesn't
 * exist yet, so Delete is hidden. `dirty` flips on the first form change
 * after load / save and clears when a save succeeds. `formKey` identifies
 * the editing session (not the record name, which changes per keystroke
 * while naming a new record) for editors that remount their form on it.
 */
export type RecordEditorState<C> =
  | { kind: "empty" }
  | { kind: "loading"; name: string }
  | {
      kind: "draft";
      content: C;
      isNew: boolean;
      dirty: boolean;
      formKey: number;
    };

export type EditorStatus = { kind: "idle" | "ok" | "err"; message: string };

const IDLE: EditorStatus = { kind: "idle", message: "" };

export interface RecordEditorConfig<C, L> {
  fetchList: () => Promise<L[]>;
  read: (name: string) => Promise<C>;
  upsert: (name: string, content: C) => Promise<unknown>;
  remove: (name: string) => Promise<unknown>;
  newDraft: () => C;
  nameOf: (content: C) => string;
  /** Noun used in the delete confirmation: `Delete <noun> "x"? …` */
  noun: string;
  /** Return an error message to refuse a save before it starts. */
  validateSave?: (draft: C, isNew: boolean, list: L[]) => string | null;
}

export function useRecordEditor<C, L>(cfg: RecordEditorConfig<C, L>) {
  const [list, setList] = useState<L[]>([]);
  const [listError, setListError] = useState<string | null>(null);
  const [editor, setEditor] = useState<RecordEditorState<C>>({ kind: "empty" });
  const [saving, setSaving] = useState(false);
  const [status, setStatus] = useState<EditorStatus>(IDLE);
  const draftSeq = useRef(0);
  // Always call the newest config: handlers are re-created each render.
  const c = useRef(cfg);
  c.current = cfg;

  const nextFormKey = () => {
    draftSeq.current += 1;
    return draftSeq.current;
  };

  const refresh = async () => {
    try {
      setList(await c.current.fetchList());
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
    setStatus(IDLE);
    try {
      const content = await c.current.read(name);
      setEditor({
        kind: "draft",
        content,
        isNew: false,
        dirty: false,
        formKey: nextFormKey(),
      });
    } catch (err) {
      setStatus({ kind: "err", message: String(err) });
      setEditor({ kind: "empty" });
    }
  };

  const startNew = () => {
    setEditor({
      kind: "draft",
      content: c.current.newDraft(),
      isNew: true,
      // A brand-new draft is dirty by default — Save is enabled the
      // moment the user types a name (the empty-name guard still
      // blocks the actual save).
      dirty: true,
      formKey: nextFormKey(),
    });
    setStatus(IDLE);
  };

  const save = async () => {
    if (editor.kind !== "draft") return;
    const refusal = c.current.validateSave?.(editor.content, editor.isNew, list);
    if (refusal) {
      setStatus({ kind: "err", message: refusal });
      return;
    }
    setSaving(true);
    setStatus(IDLE);
    try {
      await c.current.upsert(c.current.nameOf(editor.content), editor.content);
      await refresh();
      // After a successful save the file exists on disk; the draft is no
      // longer "new" and matches what's persisted. Keep the same
      // `formKey`: the content is unchanged, so remounting would only
      // cost the user their cursor position.
      setEditor({
        kind: "draft",
        content: editor.content,
        isNew: false,
        dirty: false,
        formKey: editor.formKey,
      });
      setStatus({ kind: "ok", message: "Saved." });
    } catch (err) {
      setStatus({ kind: "err", message: String(err) });
    } finally {
      setSaving(false);
    }
  };

  const remove = async () => {
    if (editor.kind !== "draft" || editor.isNew) return;
    const name = c.current.nameOf(editor.content);
    try {
      if (
        !(await confirmAction(
          `Delete ${c.current.noun} "${name}"? This cannot be undone.`,
        ))
      )
        return;
      await c.current.remove(name);
      setEditor({ kind: "empty" });
      await refresh();
      setStatus({ kind: "ok", message: "Deleted." });
    } catch (err) {
      setStatus({ kind: "err", message: String(err) });
    }
  };

  /** Edit the open draft's content and mark it dirty. */
  const change = (content: C) => {
    if (editor.kind === "draft") setEditor({ ...editor, content, dirty: true });
  };

  return {
    list,
    listError,
    editor,
    saving,
    status,
    setStatus,
    refresh,
    open,
    startNew,
    save,
    remove,
    change,
  };
}
