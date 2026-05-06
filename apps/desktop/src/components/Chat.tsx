/**
 * Chat — right-pane conversation panel.
 *
 * Renders an ordered transcript of `MessageBubble`, `ToolBadge`, and
 * "new node" dividers, plus an input row at the bottom. The "Render
 * Preview" button next to the input asks the parent to play back the
 * latest head node through the audio engine.
 *
 * Talking to Rust goes through the bridge (`sendMessage`) only — the
 * component never calls `invoke` directly. Errors from the bridge are
 * captured into the local message log as a synthetic assistant turn so
 * the user sees a friendly explanation instead of a dev-tools console
 * trace (acceptance criterion #3).
 */

import { useEffect, useRef, useState } from "react";

import { sendMessage as bridgeSendMessage } from "../lib/tauri-bridge";

import {
  useAgentStream,
  type LogEntry,
  type MessageEntry,
  type NodeDividerEntry,
  type ToolEntry,
} from "../hooks/useAgentStream";

import { MessageBubble } from "./MessageBubble";
import { ToolBadge } from "./ToolBadge";

export interface ChatProps {
  /** Whether a render-preview is currently pending. Disables the
   * button to prevent double-clicks. */
  rendering?: boolean;
  /** Called when the user clicks "Render Preview". The parent owns the
   * actual render-and-play flow because it also drives the canvas. */
  onRequestRenderPreview?: () => void;
}

function isMessage(e: LogEntry): e is MessageEntry {
  return e.kind === "message";
}
function isTool(e: LogEntry): e is ToolEntry {
  return e.kind === "tool";
}
function isNode(e: LogEntry): e is NodeDividerEntry {
  return e.kind === "node";
}

export function Chat({ rendering, onRequestRenderPreview }: ChatProps) {
  const { entries, current, pushUserMessage } = useAgentStream();
  const [input, setInput] = useState("");
  const [busy, setBusy] = useState(false);
  const [localError, setLocalError] = useState<string | null>(null);
  const scrollerRef = useRef<HTMLDivElement>(null);

  // Auto-scroll to bottom on new entries / streaming deltas.
  useEffect(() => {
    const el = scrollerRef.current;
    if (el) {
      el.scrollTop = el.scrollHeight;
    }
  }, [entries.length, current]);

  const submit = async () => {
    const text = input.trim();
    if (!text || busy) return;
    setInput("");
    setLocalError(null);
    pushUserMessage(text);
    setBusy(true);
    try {
      await bridgeSendMessage(text);
    } catch (err) {
      const friendly = friendlyError(err);
      setLocalError(friendly);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div
      data-testid="chat-root"
      className="flex h-full w-full flex-col bg-neutral-950 text-neutral-100"
    >
      <div className="flex items-center justify-between border-b border-neutral-800 px-3 py-2">
        <h2 className="text-sm font-semibold">Chat</h2>
        <button
          type="button"
          data-testid="render-preview-button"
          disabled={!!rendering}
          onClick={onRequestRenderPreview}
          className="rounded-md border border-neutral-700 bg-neutral-800 px-2 py-1 text-xs hover:bg-neutral-700 disabled:cursor-not-allowed disabled:opacity-50"
        >
          {rendering ? "Rendering…" : "Render Preview"}
        </button>
      </div>

      <div
        ref={scrollerRef}
        data-testid="chat-scroller"
        className="flex-1 space-y-2 overflow-y-auto px-3 py-3"
      >
        {entries.map((entry) => {
          if (isMessage(entry)) {
            return (
              <MessageBubble
                key={entry.id}
                role={entry.role}
                text={entry.text}
              />
            );
          }
          if (isTool(entry)) {
            return (
              <div key={entry.id} className="flex">
                <ToolBadge
                  name={entry.name}
                  status={entry.status}
                  result={entry.result}
                />
              </div>
            );
          }
          if (isNode(entry)) {
            return (
              <div
                key={entry.id}
                data-testid="node-divider"
                className="flex items-center gap-2 py-1 text-xs text-neutral-500"
              >
                <div className="h-px flex-1 bg-neutral-800" />
                <span className="font-mono">
                  new node {entry.nodeId.slice(0, 7)}
                </span>
                <div className="h-px flex-1 bg-neutral-800" />
              </div>
            );
          }
          return null;
        })}

        {current.length > 0 ? (
          <MessageBubble role="assistant" text={current} pending />
        ) : null}

        {localError ? (
          <div
            role="alert"
            data-testid="chat-error"
            className="rounded-md border border-red-800 bg-red-900/40 px-3 py-2 text-xs text-red-200"
          >
            {localError}
          </div>
        ) : null}
      </div>

      <form
        data-testid="chat-form"
        className="flex gap-2 border-t border-neutral-800 px-3 py-2"
        onSubmit={(e) => {
          e.preventDefault();
          void submit();
        }}
      >
        <input
          type="text"
          aria-label="Message"
          placeholder="Tell the agent what to edit…"
          disabled={busy}
          value={input}
          onChange={(e) => setInput(e.target.value)}
          className="flex-1 rounded-md border border-neutral-800 bg-neutral-900 px-3 py-1.5 text-sm text-neutral-100 placeholder-neutral-500 outline-none focus:border-blue-600 disabled:opacity-50"
        />
        <button
          type="submit"
          disabled={busy || input.trim().length === 0}
          className="rounded-md bg-blue-600 px-3 py-1.5 text-sm font-medium text-white hover:bg-blue-500 disabled:cursor-not-allowed disabled:opacity-50"
        >
          {busy ? "…" : "Send"}
        </button>
      </form>
    </div>
  );
}

function friendlyError(err: unknown): string {
  const raw = String(
    err instanceof Error ? err.message : (err ?? "unknown error"),
  );
  // Heuristic: the Rust side returns short kebab-ish strings; surface
  // them inline but prefix so the user knows it's a backend error.
  return `Could not complete request: ${raw}`;
}
