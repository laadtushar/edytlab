/**
 * useAgentStream — owns the live agent message + tool-call stream.
 *
 * Subscribes to the four `agent://*` events exposed through the bridge:
 *
 *  - `agent://text-delta`   → appends to `current` (the in-flight bubble)
 *  - `agent://tool-call`    → appends a running ToolBadge entry to the log
 *  - `agent://node-created` → resolves the most recent running badge to ✓
 *                             and appends a NodeDivider entry
 *  - `agent://done`         → commits `current` into the log and clears
 *
 * The hook returns a single ordered `entries` array so the Chat view can
 * render messages, tool badges, and node-created dividers interleaved in
 * the order they arrived. `pushUserMessage` is exposed for the input
 * box; the actual `sendMessage` invocation lives in the Chat component.
 *
 * Phase 1 trade-off: M11 does not forward a tool-call-end event with a
 * boolean ok flag, so we cannot mark a badge ✗ purely from the bridge.
 * Badges resolve to ✓ on `node-created` and stay "running" otherwise.
 * If the agent reports a tool failure in plain text the user still sees
 * it in the message bubble. Tracked for an M11 follow-up.
 */

import { useCallback, useEffect, useRef, useState } from "react";

import {
  approvePlan as bridgeApprovePlan,
  onAgentDone,
  onNodeCreated,
  onPlan,
  onTextDelta,
  onToolCall,
  type NodeId,
} from "../lib/tauri-bridge";

export type ChatRole = "user" | "assistant";

export type ToolStatus = "running" | "ok" | "error";

export interface MessageEntry {
  kind: "message";
  id: string;
  role: ChatRole;
  text: string;
}

export interface ToolEntry {
  kind: "tool";
  id: string;
  name: string;
  status: ToolStatus;
  /** Optional human-friendly result text (e.g. "normalized -1 dBFS"). */
  result?: string;
}

export interface NodeDividerEntry {
  kind: "node";
  id: string;
  nodeId: NodeId;
}

export interface PlanEntry {
  kind: "plan";
  id: string;
  steps: Array<{ step: number; tool: string; description: string }>;
}

export type LogEntry = MessageEntry | ToolEntry | NodeDividerEntry | PlanEntry;

export interface UseAgentStreamResult {
  /** Ordered transcript of messages, tool badges, and node dividers. */
  entries: LogEntry[];
  /** The streaming assistant message, if any. Empty string when idle. */
  current: string;
  /** Append a user message locally (the caller is responsible for
   * forwarding it through the bridge via `sendMessage`). */
  pushUserMessage: (text: string) => void;
  /** Reset the transcript, e.g. when switching projects. */
  reset: () => void;
  /** Non-null while the agent is awaiting plan approval (mashup mode). */
  pendingPlan: PlanEntry | null;
  /** Approve the pending plan. Clears `pendingPlan` and unblocks the loop. */
  approvePlan: () => Promise<void>;
}

let _idCounter = 0;
const nextId = (): string => {
  _idCounter += 1;
  return `e${_idCounter}`;
};

export function useAgentStream(): UseAgentStreamResult {
  const [entries, setEntries] = useState<LogEntry[]>([]);
  const [current, setCurrent] = useState<string>("");
  const [pendingPlan, setPendingPlan] = useState<PlanEntry | null>(null);

  // Track the in-flight assistant text in a ref so the `done` listener
  // sees the latest value without needing to be re-bound on every delta.
  const currentRef = useRef<string>("");

  useEffect(() => {
    let unlistenDelta: (() => void) | null = null;
    let unlistenTool: (() => void) | null = null;
    let unlistenNode: (() => void) | null = null;
    let unlistenDone: (() => void) | null = null;
    let unlistenPlan: (() => void) | null = null;
    let cancelled = false;

    const attach = (
      p: Promise<() => void>,
      assign: (fn: () => void) => void,
    ) => {
      p.then((fn) => {
        if (cancelled) {
          fn();
        } else {
          assign(fn);
        }
      });
    };

    attach(
      onTextDelta((text) => {
        currentRef.current += text;
        setCurrent(currentRef.current);
      }),
      (fn) => {
        unlistenDelta = fn;
      },
    );

    attach(
      onToolCall((name, id) => {
        setEntries((prev) => [
          ...prev,
          { kind: "tool", id, name, status: "running" },
        ]);
      }),
      (fn) => {
        unlistenTool = fn;
      },
    );

    attach(
      onNodeCreated((nodeId) => {
        setEntries((prev) => {
          const next = [...prev];
          // Resolve the most recent running tool badge to ok.
          for (let i = next.length - 1; i >= 0; i -= 1) {
            const e = next[i];
            if (e.kind === "tool" && e.status === "running") {
              next[i] = { ...e, status: "ok" };
              break;
            }
          }
          next.push({ kind: "node", id: nextId(), nodeId });
          return next;
        });
      }),
      (fn) => {
        unlistenNode = fn;
      },
    );

    attach(
      onAgentDone(() => {
        const text = currentRef.current;
        if (text.length > 0) {
          setEntries((prev) => [
            ...prev,
            { kind: "message", id: nextId(), role: "assistant", text },
          ]);
        }
        currentRef.current = "";
        setCurrent("");
      }),
      (fn) => {
        unlistenDone = fn;
      },
    );

    attach(
      onPlan((rawSteps) => {
        // Coerce each step object to the typed PlanEntry shape.
        const steps = rawSteps.map((s) => ({
          step: (s["step"] as number) ?? 0,
          tool: (s["tool"] as string) ?? "",
          description: (s["description"] as string) ?? "",
        }));
        const entry: PlanEntry = { kind: "plan", id: crypto.randomUUID(), steps };
        setEntries((prev) => [...prev, entry]);
        setPendingPlan(entry);
      }),
      (fn) => {
        unlistenPlan = fn;
      },
    );

    return () => {
      cancelled = true;
      unlistenDelta?.();
      unlistenTool?.();
      unlistenNode?.();
      unlistenDone?.();
      unlistenPlan?.();
    };
  }, []);

  const pushUserMessage = useCallback((text: string) => {
    setEntries((prev) => [
      ...prev,
      { kind: "message", id: nextId(), role: "user", text },
    ]);
  }, []);

  const reset = useCallback(() => {
    setEntries([]);
    setCurrent("");
    currentRef.current = "";
    setPendingPlan(null);
  }, []);

  const approvePlan = useCallback(async () => {
    await bridgeApprovePlan();
    setPendingPlan(null);
  }, []);

  return { entries, current, pushUserMessage, reset, pendingPlan, approvePlan };
}
