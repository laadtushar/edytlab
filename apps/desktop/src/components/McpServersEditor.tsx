/**
 * McpServersEditor — CRUD UI for `~/.edytlab/mcp.json`.
 *
 * Servers are stdio (local subprocess) or SSE (remote URL). The list
 * shows status (stopped / running / error) and tool count when
 * running. Edit form has transport-specific fields plus env / headers.
 *
 * Secrets in env values use the `<keychain:slot>` placeholder; the
 * editor renders these verbatim — the Rust side resolves them at
 * server-launch time.
 *
 * The form is keyed by the editing session (`formKey` in
 * `useRecordEditor`), so opening a different server remounts it and
 * re-derives its raw text; it does not track the id, which changes on
 * every keystroke while naming a new server.
 */

import { useState } from "react";

import {
  deleteMcpServer,
  listMcpServers,
  readMcpServer,
  restartMcpServer,
  upsertMcpServer,
  type McpServerEntry,
  type McpServerListEntry,
  type McpTransport,
} from "../lib/tauri-bridge";
import { IDENTIFIER_INPUT } from "../lib/identifierInput";
import { useRecordEditor } from "../lib/useRecordEditor";
import {
  EditorBody,
  EditorFooter,
  EditorLayout,
  Row,
} from "./RecordEditorParts";

const DRAFT_DEFAULT: McpServerEntry = {
  id: "",
  transport: "stdio",
  command: "",
  args: [],
  env: {},
  url: "",
  headers: {},
  enabled: true,
};

export function McpServersEditor() {
  const ed = useRecordEditor<McpServerEntry, McpServerListEntry>({
    fetchList: listMcpServers,
    read: readMcpServer,
    upsert: upsertMcpServer,
    remove: deleteMcpServer,
    newDraft: () => ({ ...DRAFT_DEFAULT }),
    nameOf: (e) => e.id,
    noun: "server",
  });
  const { editor, list } = ed;

  const handleRestart = async (id: string) => {
    try {
      await restartMcpServer(id);
      await ed.refresh();
      ed.setStatus({ kind: "ok", message: `Restarted ${id}.` });
    } catch (err) {
      ed.setStatus({ kind: "err", message: String(err) });
      await ed.refresh();
    }
  };

  return (
    <EditorLayout
      testId="mcp-servers-editor"
      asideWidth="w-48"
      title="MCP servers"
      newTestId="mcp-new"
      onNew={ed.startNew}
      listError={ed.listError}
      listErrorTestId="mcp-list-error"
      isEmpty={list.length === 0}
      emptyTestId="mcp-list-empty"
      emptyText='No servers registered yet. Click "New" or edit ~/.edytlab/mcp.json.'
      list={
        <ul>
          {list.map((s) => {
            const isEditingRow =
              editor.kind === "draft" && editor.content.id === s.id;
            return (
              <li
                key={s.id}
                className={
                  "border-b border-[var(--border)] " +
                  (isEditingRow ? "bg-[var(--accent-soft)]" : "")
                }
              >
                <button
                  type="button"
                  data-testid={`mcp-row-${s.id}`}
                  onClick={() => void ed.open(s.id)}
                  className={
                    "block w-full truncate px-2 py-1.5 text-left text-xs transition " +
                    (isEditingRow
                      ? "text-[var(--accent-text)]"
                      : "text-[var(--text-dim)] hover:text-[var(--text)]")
                  }
                >
                  <div className="flex items-center justify-between gap-1">
                    <span className="truncate font-mono text-[11px]">
                      {s.id}
                    </span>
                    <StatusBadge status={s.status} />
                  </div>
                  <div className="truncate text-[10px] text-[var(--text-faint)]">
                    {s.transport}
                    {s.status === "running" ? ` · ${s.tools_count} tools` : ""}
                  </div>
                </button>
                <button
                  type="button"
                  data-testid={`mcp-restart-${s.id}`}
                  onClick={() => void handleRestart(s.id)}
                  className="ml-2 mb-1 btn-secondary rounded px-1.5 py-0 text-[9px] uppercase tracking-wider transition"
                >
                  Restart
                </button>
              </li>
            );
          })}
        </ul>
      }
    >
      <EditorBody
        editor={editor}
        emptyTestId="mcp-editor-empty"
        emptyClassName="flex h-full items-center justify-center rounded-md border border-dashed border-[var(--border)] text-xs text-[var(--text-faint)]"
        empty='Select a server or click "New" to add one.'
        form={(d) => (
          <McpServerForm
            key={d.formKey}
            entry={d.content}
            isNew={d.isNew}
            onChange={ed.change}
          />
        )}
      />
      <EditorFooter
        prefix="mcp"
        editor={editor}
        status={ed.status}
        saving={ed.saving}
        nameOf={(e) => e.id}
        onDelete={() => void ed.remove()}
        onSave={() => void ed.save()}
      />
    </EditorLayout>
  );
}

function StatusBadge({ status }: { status: McpServerListEntry["status"] }) {
  const map: Record<McpServerListEntry["status"], string> = {
    running: "var(--success)",
    stopped: "var(--text-faint)",
    error: "var(--danger)",
  };
  return (
    <span
      className="rounded px-1 text-[9px] uppercase tracking-wider"
      style={{ color: map[status] }}
      data-testid={`mcp-status-badge-${status}`}
    >
      {status}
    </span>
  );
}

interface FormProps {
  entry: McpServerEntry;
  isNew: boolean;
  onChange: (e: McpServerEntry) => void;
}

function McpServerForm({ entry, isNew, onChange }: FormProps) {
  // `args`, `env`, and `headers` are stored parsed but edited as text,
  // and the parsed forms cannot represent an edit in progress: a key
  // typed before its `=` parses to nothing, and a just-pressed newline
  // is an empty entry that any sane parse drops. Rendering the textarea
  // from the parsed value therefore deletes characters as fast as they
  // are typed. Raw text lives here and drives what the user sees; the
  // parsed value is pushed to the parent on every change so Save stays
  // correct. The parent remounts this form when a different server is
  // opened, so deriving the initial text once from props is enough.
  const [argsText, setArgsText] = useState(() => entry.args.join("\n"));
  const [envText, setEnvText] = useState(() => kvToText(entry.env, "="));
  const [headersText, setHeadersText] = useState(() =>
    kvToText(entry.headers, ": "),
  );

  return (
    <div className="flex flex-1 flex-col gap-2 overflow-y-auto pr-1">
      <Row label="ID">
        <input
          {...IDENTIFIER_INPUT}
          type="text"
          data-testid="mcp-id"
          value={entry.id}
          disabled={!isNew}
          onChange={(e) => onChange({ ...entry, id: e.target.value })}
          placeholder="github"
          className="field w-full px-2 py-1 font-mono text-xs outline-none transition disabled:cursor-not-allowed disabled:opacity-60"
        />
      </Row>
      <Row label="Transport">
        <select
          data-testid="mcp-transport"
          value={entry.transport}
          onChange={(e) =>
            onChange({
              ...entry,
              transport: e.target.value as McpTransport,
            })
          }
          className="field w-full px-2 py-1 text-xs outline-none transition"
        >
          <option value="stdio">stdio (local subprocess)</option>
          <option value="sse">remote (HTTP)</option>
        </select>
      </Row>
      {entry.transport === "stdio" ? (
        <>
          <Row label="Command">
            <input
              {...IDENTIFIER_INPUT}
              type="text"
              data-testid="mcp-command"
              value={entry.command}
              onChange={(e) =>
                onChange({ ...entry, command: e.target.value })
              }
              placeholder="npx"
              className="field w-full px-2 py-1 font-mono text-xs outline-none transition"
            />
          </Row>
          <Row label="Args (one per line)">
            <textarea
              {...IDENTIFIER_INPUT}
              data-testid="mcp-args"
              value={argsText}
              onChange={(e) => {
                setArgsText(e.target.value);
                onChange({ ...entry, args: parseArgs(e.target.value) });
              }}
              rows={3}
              placeholder="-y\n@modelcontextprotocol/server-github"
              className="field w-full resize-y px-2 py-1 font-mono text-xs outline-none transition"
            />
          </Row>
          <Row label="Env (KEY=value per line; secrets as <keychain:slot>)">
            <textarea
              {...IDENTIFIER_INPUT}
              data-testid="mcp-env"
              value={envText}
              onChange={(e) => {
                setEnvText(e.target.value);
                onChange({ ...entry, env: parseKv(e.target.value) });
              }}
              rows={3}
              placeholder="GITHUB_TOKEN=<keychain:github_token>"
              className="field w-full resize-y px-2 py-1 font-mono text-xs outline-none transition"
            />
          </Row>
        </>
      ) : (
        <>
          <Row label="URL">
            <input
              {...IDENTIFIER_INPUT}
              type="text"
              data-testid="mcp-url"
              value={entry.url}
              onChange={(e) => onChange({ ...entry, url: e.target.value })}
              placeholder="https://mcp.example.com/mcp"
              className="field w-full px-2 py-1 font-mono text-xs outline-none transition"
            />
          </Row>
          <Row label="Headers (Name: value per line)">
            <textarea
              {...IDENTIFIER_INPUT}
              data-testid="mcp-headers"
              value={headersText}
              onChange={(e) => {
                setHeadersText(e.target.value);
                onChange({ ...entry, headers: parseKv(e.target.value, ":") });
              }}
              rows={3}
              className="field w-full resize-y px-2 py-1 font-mono text-xs outline-none transition"
            />
          </Row>
        </>
      )}
      <Row label="Enabled">
        <label className="flex items-center gap-1.5 text-xs text-[var(--text-dim)]">
          <input
            type="checkbox"
            data-testid="mcp-enabled"
            checked={entry.enabled}
            onChange={(e) => onChange({ ...entry, enabled: e.target.checked })}
          />
          on
        </label>
      </Row>
    </div>
  );
}

/** One argument per line; blanks and surrounding space are dropped. */
function parseArgs(text: string): string[] {
  return text
    .split("\n")
    .map((s) => s.trim())
    .filter(Boolean);
}

/** Inverse of {@link parseKv}, for seeding a textarea from stored pairs. */
function kvToText(kv: Record<string, string>, sep: "=" | ": "): string {
  return Object.entries(kv)
    .map(([k, v]) => `${k}${sep}${v}`)
    .join("\n");
}

function parseKv(text: string, sep: "=" | ":" = "="): Record<string, string> {
  const out: Record<string, string> = {};
  for (const line of text.split("\n")) {
    const trimmed = line.trim();
    if (!trimmed) continue;
    const idx = trimmed.indexOf(sep);
    if (idx < 1) continue;
    const k = trimmed.slice(0, idx).trim();
    const v = trimmed.slice(idx + 1).trim();
    if (k) out[k] = v;
  }
  return out;
}
