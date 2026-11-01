/**
 * Shared layout for the Settings record editors (skills, agent profiles,
 * MCP servers): the list pane, the empty / loading / form body, the status
 * and Save / Delete footer, and the labelled form `Row`.
 */

import type { ReactNode } from "react";

import { IDENTIFIER_INPUT } from "../lib/identifierInput";

import type {
  EditorStatus,
  RecordEditorState,
} from "../lib/useRecordEditor";

export function Row({
  label,
  children,
}: {
  label: string;
  children: ReactNode;
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

/** Whole editor: list pane on the left, `children` (the right side) after. */
export function EditorLayout({
  testId,
  asideWidth,
  title,
  newTestId,
  onNew,
  listError,
  listErrorTestId,
  isEmpty,
  emptyTestId,
  emptyText,
  list,
  children,
}: {
  testId: string;
  asideWidth: "w-44" | "w-48";
  title: string;
  newTestId: string;
  onNew: () => void;
  listError: string | null;
  listErrorTestId: string;
  isEmpty: boolean;
  emptyTestId: string;
  emptyText: string;
  /** The `<ul>` shown when the list has rows. */
  list: ReactNode;
  children: ReactNode;
}) {
  return (
    <div data-testid={testId} className="flex h-[28rem] gap-3">
      <aside
        className={`${asideWidth} flex-shrink-0 overflow-y-auto rounded-md border border-[var(--border-strong)] bg-[var(--surface)]`}
      >
        <div className="flex items-center justify-between border-b border-[var(--border)] px-2 py-1.5">
          <span className="font-mono text-[10px] uppercase tracking-[0.18em] text-[var(--text-faint)]">
            {title}
          </span>
          <button
            type="button"
            data-testid={newTestId}
            onClick={onNew}
            className="btn-secondary rounded px-2 py-0.5 text-[10px] uppercase tracking-wider transition"
          >
            New
          </button>
        </div>
        {listError ? (
          <p
            data-testid={listErrorTestId}
            className="px-2 py-1 text-[11px] text-[var(--danger)]"
          >
            {listError}
          </p>
        ) : isEmpty ? (
          <p
            data-testid={emptyTestId}
            className="px-2 py-2 text-[11px] italic text-[var(--text-faint)]"
          >
            {emptyText}
          </p>
        ) : (
          list
        )}
      </aside>
      <section className="flex flex-1 flex-col gap-2">{children}</section>
    </div>
  );
}

/** Right-hand body: empty placeholder, loading line, or the form. */
export function EditorBody<C>({
  editor,
  emptyTestId,
  empty,
  emptyClassName,
  form,
}: {
  editor: RecordEditorState<C>;
  emptyTestId: string;
  empty: ReactNode;
  emptyClassName: string;
  form: (draft: Extract<RecordEditorState<C>, { kind: "draft" }>) => ReactNode;
}) {
  if (editor.kind === "empty") {
    return (
      <div data-testid={emptyTestId} className={emptyClassName}>
        {empty}
      </div>
    );
  }
  if (editor.kind === "loading") {
    return (
      <div className="flex h-full items-center justify-center text-xs text-[var(--text-faint)]">
        Loading {editor.name}…
      </div>
    );
  }
  return <>{form(editor)}</>;
}

export function EditorFooter<C>({
  prefix,
  editor,
  status,
  saving,
  nameOf,
  onDelete,
  onSave,
}: {
  /** data-testid prefix: `<prefix>-status`, `-delete`, `-save`. */
  prefix: string;
  editor: RecordEditorState<C>;
  status: EditorStatus;
  saving: boolean;
  nameOf: (c: C) => string;
  onDelete: () => void;
  onSave: () => void;
}) {
  // Save is off unless a dirty draft with a non-blank name is not mid-save.
  const saveDisabled =
    editor.kind !== "draft" ||
    saving ||
    !editor.dirty ||
    nameOf(editor.content).trim() === "";
  return (
    <div className="flex items-center justify-between gap-2">
      <span
        data-testid={`${prefix}-status`}
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
            data-testid={`${prefix}-delete`}
            onClick={onDelete}
            className="btn-danger rounded-md px-3 py-1 text-xs transition"
          >
            Delete
          </button>
        ) : null}
        <button
          type="button"
          data-testid={`${prefix}-save`}
          disabled={saveDisabled}
          onClick={onSave}
          className="btn-primary px-3 py-1 text-xs transition"
        >
          {saving ? "Saving…" : "Save"}
        </button>
      </div>
    </div>
  );
}

/** The Name row: identifier input, locked once the record exists. */
export function NameRow({
  prefix,
  value,
  locked,
  placeholder,
  onChange,
}: {
  prefix: string;
  value: string;
  locked: boolean;
  placeholder: string;
  onChange: (v: string) => void;
}) {
  return (
    <Row label="Name">
      <input
        {...IDENTIFIER_INPUT}
        type="text"
        data-testid={`${prefix}-name`}
        value={value}
        disabled={locked}
        onChange={(e) => onChange(e.target.value)}
        placeholder={placeholder}
        className="field w-full px-2 py-1 font-mono text-xs outline-none transition disabled:cursor-not-allowed disabled:opacity-60"
      />
    </Row>
  );
}

export function DescriptionRow({
  prefix,
  value,
  onChange,
}: {
  prefix: string;
  value: string;
  onChange: (v: string) => void;
}) {
  return (
    <Row label="Description">
      <input
        type="text"
        data-testid={`${prefix}-description`}
        value={value}
        onChange={(e) => onChange(e.target.value)}
        className="field w-full px-2 py-1 text-xs outline-none transition"
      />
    </Row>
  );
}

/** The 10-row monospace body textarea. */
export function BodyRow({
  label,
  prefix,
  value,
  placeholder,
  onChange,
}: {
  label: string;
  prefix: string;
  value: string;
  placeholder?: string;
  onChange: (v: string) => void;
}) {
  return (
    <Row label={label}>
      <textarea
        data-testid={`${prefix}-body`}
        value={value}
        onChange={(e) => onChange(e.target.value)}
        rows={10}
        placeholder={placeholder}
        className="field w-full resize-y px-2 py-1 font-mono text-xs outline-none transition"
      />
    </Row>
  );
}
