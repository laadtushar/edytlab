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
 */

import { useEffect, useRef, useState } from "react";

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

type EditorState =
  | { kind: "empty" }
  | { kind: "loading"; id: string }
  | {
      kind: "draft";
      entry: McpServerEntry;
      isNew: boolean;
      dirty: boolean;
      /**
       * Identity of this editing session, used as the form's React
       * `key` so opening a different server remounts the form and
       * re-derives its raw text from the new entry. It deliberately
       * does NOT track `entry.id` — that changes on every keystroke
       * while naming a new server, and remounting mid-edit would steal
       * focus from the ID input.
       */
      formKey: number;
    };

export function McpServersEditor() {
  const [list, setList] = useState<McpServerListEntry[]>([]);
  const [listError, setListError] = useState<string | null>(null);
  const [editor, setEditor] = useState<EditorState>({ kind: "empty" });
  const [saving, setSaving] = useState(false);
  const [status, setStatus] = useState<{
    kind: "idle" | "ok" | "err";
    message: string;
  }>({ kind: "idle", message: "" });
  // Monotonic id handed to each new editing session; see `formKey`.
  const draftSeq = useRef(0);

  const nextFormKey = () => {
    draftSeq.current += 1;
    return draftSeq.current;
  };

  const refresh = async () => {
    try {
      const l = await listMcpServers();
      setList(l);
      setListError(null);
    } catch (err) {
      setListError(String(err));
    }
  };

  useEffect(() => {
    void refresh();
  }, []);

  const open = async (id: string) => {
    setEditor({ kind: "loading", id });
    setStatus({ kind: "idle", message: "" });
    try {
      const entry = await readMcpServer(id);
      setEditor({
        kind: "draft",
        entry,
        isNew: false,
        dirty: false,
        formKey: nextFormKey(),
      });
    } catch (err) {
      setStatus({ kind: "err", message: String(err) });
      setEditor({ kind: "empty" });
    }
  };

  const newServer = () => {
    setEditor({
      kind: "draft",
      entry: { ...DRAFT_DEFAULT },
      isNew: true,
      dirty: true,
      formKey: nextFormKey(),
    });
    setStatus({ kind: "idle", message: "" });
  };

  const handleSave = async () => {
    if (editor.kind !== "draft") return;
    setSaving(true);
    setStatus({ kind: "idle", message: "" });
    try {
      await upsertMcpServer(editor.entry.id, editor.entry);
      await refresh();
      // Keep the same `formKey`: the entry is unchanged by a save, so
      // remounting would only cost the user their cursor position.
      setEditor({
        kind: "draft",
        entry: editor.entry,
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

  const handleDelete = async () => {
    if (editor.kind !== "draft" || editor.isNew) return;
    if (
      !window.confirm(
        `Delete server "${editor.entry.id}"? This cannot be undone.`,
      )
    )
      return;
    try {
      await deleteMcpServer(editor.entry.id);
      setEditor({ kind: "empty" });
      await refresh();
      setStatus({ kind: "ok", message: "Deleted." });
    } catch (err) {
      setStatus({ kind: "err", message: String(err) });
    }
  };

  const handleRestart = async (id: string) => {
    try {
      await restartMcpServer(id);
      await refresh();
      setStatus({ kind: "ok", message: `Restarted ${id}.` });
    } catch (err) {
      setStatus({ kind: "err", message: String(err) });
      await refresh();
    }
  };

  return (
    <div data-testid="mcp-servers-editor" className="flex h-[28rem] gap-3">
      <aside className="w-48 flex-shrink-0 overflow-y-auto rounded-md border border-[var(--border-strong)] bg-[var(--surface)]">
        <div className="flex items-center justify-between border-b border-[var(--border)] px-2 py-1.5">
          <span className="font-mono text-[10px] uppercase tracking-[0.18em] text-[var(--text-faint)]">
            MCP servers
          </span>
          <button
            type="button"
            data-testid="mcp-new"
            onClick={newServer}
            className="rounded border border-[var(--border-strong)] px-2 py-0.5 text-[10px] uppercase tracking-wider text-[var(--text-dim)] transition hover:border-[var(--accent)]/50 hover:text-[var(--accent)]"
          >
            New
          </button>
        </div>
        {listError ? (
          <p
            data-testid="mcp-list-error"
            className="px-2 py-1 text-[11px] text-[var(--danger)]"
          >
            {listError}
          </p>
        ) : list.length === 0 ? (
          <p
            data-testid="mcp-list-empty"
            className="px-2 py-2 text-[11px] italic text-[var(--text-faint)]"
          >
            No servers registered yet. Click "New" or edit ~/.edytlab/mcp.json.
          </p>
        ) : (
          <ul>
            {list.map((s) => {
              const isEditingRow =
                editor.kind === "draft" && editor.entry.id === s.id;
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
                    onClick={() => void open(s.id)}
                    className={
                      "block w-full truncate px-2 py-1.5 text-left text-xs transition " +
                      (isEditingRow
                        ? "text-[var(--accent)]"
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
                    className="ml-2 mb-1 rounded border border-[var(--border-strong)] px-1.5 py-0 text-[9px] uppercase tracking-wider text-[var(--text-dim)] transition hover:border-[var(--accent)]/50 hover:text-[var(--accent)]"
                  >
                    Restart
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
            data-testid="mcp-editor-empty"
            className="flex h-full items-center justify-center rounded-md border border-dashed border-[var(--border)] text-xs text-[var(--text-faint)]"
          >
            Select a server or click "New" to add one.
          </div>
        ) : editor.kind === "loading" ? (
          <div className="flex h-full items-center justify-center text-xs text-[var(--text-faint)]">
            Loading {editor.id}…
          </div>
        ) : (
          <McpServerForm
            key={editor.formKey}
            entry={editor.entry}
            isNew={editor.isNew}
            onChange={(e) => setEditor({ ...editor, entry: e, dirty: true })}
          />
        )}

        <div className="flex items-center justify-between gap-2">
          <span
            data-testid="mcp-status"
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
                data-testid="mcp-delete"
                onClick={() => void handleDelete()}
                className="rounded-md border border-[var(--danger)]/40 px-3 py-1 text-xs text-[var(--danger)] transition hover:bg-[var(--danger)]/10"
              >
                Delete
              </button>
            ) : null}
            <button
              type="button"
              data-testid="mcp-save"
              disabled={
                editor.kind !== "draft" ||
                saving ||
                !editor.dirty ||
                editor.entry.id.trim() === ""
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
          type="text"
          data-testid="mcp-id"
          value={entry.id}
          disabled={!isNew}
          onChange={(e) => onChange({ ...entry, id: e.target.value })}
          placeholder="github"
          className="w-full rounded-md border border-[var(--border-strong)] bg-[var(--surface)] px-2 py-1 font-mono text-xs text-[var(--text)] outline-none transition focus:border-[var(--accent)]/55 focus:shadow-[0_0_0_3px_var(--accent-soft)] disabled:cursor-not-allowed disabled:opacity-60"
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
          className="w-full rounded-md border border-[var(--border-strong)] bg-[var(--surface)] px-2 py-1 text-xs text-[var(--text)] outline-none transition focus:border-[var(--accent)]/55"
        >
          <option value="stdio">stdio (local subprocess)</option>
          <option value="sse">remote (HTTP)</option>
        </select>
      </Row>
      {entry.transport === "stdio" ? (
        <>
          <Row label="Command">
            <input
              type="text"
              data-testid="mcp-command"
              value={entry.command}
              onChange={(e) =>
                onChange({ ...entry, command: e.target.value })
              }
              placeholder="npx"
              className="w-full rounded-md border border-[var(--border-strong)] bg-[var(--surface)] px-2 py-1 font-mono text-xs text-[var(--text)] outline-none transition focus:border-[var(--accent)]/55"
            />
          </Row>
          <Row label="Args (one per line)">
            <textarea
              data-testid="mcp-args"
              value={argsText}
              onChange={(e) => {
                setArgsText(e.target.value);
                onChange({ ...entry, args: parseArgs(e.target.value) });
              }}
              rows={3}
              placeholder="-y\n@modelcontextprotocol/server-github"
              className="w-full resize-y rounded-md border border-[var(--border-strong)] bg-[var(--surface)] px-2 py-1 font-mono text-xs text-[var(--text)] outline-none transition focus:border-[var(--accent)]/55"
            />
          </Row>
          <Row label="Env (KEY=value per line; secrets as <keychain:slot>)">
            <textarea
              data-testid="mcp-env"
              value={envText}
              onChange={(e) => {
                setEnvText(e.target.value);
                onChange({ ...entry, env: parseKv(e.target.value) });
              }}
              rows={3}
              placeholder="GITHUB_TOKEN=<keychain:github_token>"
              className="w-full resize-y rounded-md border border-[var(--border-strong)] bg-[var(--surface)] px-2 py-1 font-mono text-xs text-[var(--text)] outline-none transition focus:border-[var(--accent)]/55"
            />
          </Row>
        </>
      ) : (
        <>
          <Row label="URL">
            <input
              type="text"
              data-testid="mcp-url"
              value={entry.url}
              onChange={(e) => onChange({ ...entry, url: e.target.value })}
              placeholder="https://mcp.example.com/mcp"
              className="w-full rounded-md border border-[var(--border-strong)] bg-[var(--surface)] px-2 py-1 font-mono text-xs text-[var(--text)] outline-none transition focus:border-[var(--accent)]/55"
            />
          </Row>
          <Row label="Headers (Name: value per line)">
            <textarea
              data-testid="mcp-headers"
              value={headersText}
              onChange={(e) => {
                setHeadersText(e.target.value);
                onChange({ ...entry, headers: parseKv(e.target.value, ":") });
              }}
              rows={3}
              className="w-full resize-y rounded-md border border-[var(--border-strong)] bg-[var(--surface)] px-2 py-1 font-mono text-xs text-[var(--text)] outline-none transition focus:border-[var(--accent)]/55"
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
