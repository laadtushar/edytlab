/**
 * useAgentStream — owns the live agent message + tool-call stream.
 *
 * Subscribes to the `agent://*` events exposed through the bridge:
 *
 *  - `agent://text-delta`     → appends to `current` (the in-flight bubble)
 *  - `agent://tool-call`      → appends a running ToolBadge entry to the log
 *  - `agent://tool-call-end`  → resolves that badge to ok / error by id
 *  - `agent://node-created`   → appends a NodeDivider entry
 *  - `agent://done`           → commits `current` into the log, derives
 *                                chips for the just-finished assistant
 *                                turn, and clears the streaming buffer
 *  - `agent://plan`           → records a pending plan-approval card
 *
 * `awaiting` is the public flag a UI uses to show a "Thinking…" pill
 * between the user's submit and the first downstream event. The hook
 * sets it via `pushUserMessage` and clears it on the first delta,
 * tool-call, plan, or done event of the turn.
 *
 * Tool-call resolution: each tool-call event carries an `id`; the
 * matching tool-call-end event uses the same id and an `ok: boolean`.
 * Older code resolved badges off `node-created` only, which can't
 * disambiguate ok vs error and ignores non-node-producing tools (e.g.
 * `render_preview`). We now resolve by id so any tool can finish with
 * the right status. `node-created` is still rendered as a divider for
 * graph visibility.
 */

import { useCallback, useEffect, useRef, useState } from "react";

import {
  approvePlan as bridgeApprovePlan,
  onAgentDone,
  onNodeCreated,
  onPlan,
  onTextDelta,
  onToolCall,
  onToolCallEnd,
  type NodeId,
  type ToolView,
} from "../lib/tauri-bridge";

export type { ToolView, SpectrumPoint } from "../lib/tauri-bridge";

export type ChatRole = "user" | "assistant";

export type ToolStatus = "running" | "ok" | "error";

/**
 * A one-click action attached to an assistant message. The user can
 * tap the chip to submit `prompt` as the next user turn — exactly as
 * if they had typed it themselves. Chips are derived locally from the
 * assistant's natural-language reply (see {@link deriveChips}); the
 * backend does not emit them today, but the surface is here so we can
 * move derivation server-side without changing the UI.
 */
export interface Chip {
  id: string;
  label: string;
  icon: ChipIcon;
  prompt: string;
}

/** Symbolic icon identifiers; `MessageBubble` maps these to SVGs. */
export type ChipIcon = "play" | "save" | "wand" | "scissors" | "undo";

export interface MessageEntry {
  kind: "message";
  id: string;
  role: ChatRole;
  text: string;
  /** Action chips, only meaningful on assistant messages. */
  chips?: Chip[];
}

export interface ToolEntry {
  kind: "tool";
  id: string;
  name: string;
  status: ToolStatus;
  /** Optional human-friendly result text (e.g. "normalized -1 dBFS"). */
  result?: string;
  /**
   * Chart data from the tool's result, for the tools that produce one.
   * Arrives on the `tool-call-end` event and renders under the badge.
   */
  view?: ToolView;
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
  /**
   * True between the user's submit and the first downstream event of
   * the turn (delta / tool-call / plan / done). Drives the "Thinking…"
   * pulse.
   */
  awaiting: boolean;
  /** Append a user message locally (the caller is responsible for
   * forwarding it through the bridge via `sendMessage`). */
  pushUserMessage: (text: string) => void;
  /** Reset the transcript, e.g. when switching projects. */
  reset: () => void;
  /** Non-null while the agent is awaiting plan approval (mashup mode). */
  pendingPlan: PlanEntry | null;
  /** Approve the pending plan. Clears `pendingPlan` and unblocks the loop.
   *  Pass `steps` when the user edited one or more step descriptions; the
   *  override is forwarded to the backend so the agent follows the revised
   *  plan rather than the original one. */
  approvePlan: (steps?: Array<{ step: number; tool: string; description: string }>) => Promise<void>;
}

/**
 * Derive at-most-4 action chips from an assistant message body.
 *
 * v1 strategy: scan the message for known tool names (the same names
 * the dispatcher emits via `tool_schemas`). The first match for each
 * tool wins; we cap at four so the chip row stays one line on
 * reasonable widths. Order follows {@link CHIP_DEFS} so the most
 * common follow-up actions are leftmost.
 *
 * Exported for tests; not part of the hook's stable surface.
 */
export function deriveChips(text: string): Chip[] {
  const out: Chip[] = [];
  const lower = text.toLowerCase();
  for (const def of CHIP_DEFS) {
    if (out.length >= 4) break;
    if (def.match.some((m) => lower.includes(m))) {
      out.push({
        id: `${def.tool}-${out.length}`,
        label: def.label,
        icon: def.icon,
        prompt: def.prompt,
      });
    }
  }
  return out;
}

interface ChipDef {
  tool: string;
  match: string[];
  label: string;
  icon: ChipIcon;
  prompt: string;
}

/**
 * The static menu of chips we know how to derive. New tools land here
 * with the natural-language phrases the model tends to use ("preview",
 * "render"), the chip label, an icon id, and the prompt we send back.
 */
const CHIP_DEFS: ChipDef[] = [
  {
    tool: "render_preview",
    match: ["render_preview", "preview"],
    label: "Preview",
    icon: "play",
    prompt: "Preview the current result.",
  },
  {
    tool: "render_final",
    match: ["render_final", "final wav", "render a final"],
    label: "Render final",
    icon: "save",
    prompt: "Render the final WAV file.",
  },
  {
    tool: "normalize",
    match: ["normalize"],
    label: "Normalize",
    icon: "wand",
    prompt: "Normalize to -1 dBFS.",
  },
  {
    tool: "trim",
    match: ["trim"],
    label: "Trim",
    icon: "scissors",
    prompt: "Trim silence from the start and end.",
  },
  {
    tool: "revert_to",
    match: ["revert_to", "undo"],
    label: "Undo",
    icon: "undo",
    prompt: "Revert to the previous session node.",
  },
];

let _idCounter = 0;
const nextId = (): string => {
  _idCounter += 1;
  return `e${_idCounter}`;
};

export function useAgentStream(): UseAgentStreamResult {
  const [entries, setEntries] = useState<LogEntry[]>([]);
  const [current, setCurrent] = useState<string>("");
  const [pendingPlan, setPendingPlan] = useState<PlanEntry | null>(null);
  const [awaiting, setAwaiting] = useState<boolean>(false);

  // Track the in-flight assistant text in a ref so the `done` listener
  // sees the latest value without needing to be re-bound on every delta.
  const currentRef = useRef<string>("");

  // Any event from the agent ends the "Thinking…" pulse. Centralised so
  // each listener doesn't have to remember to flip it.
  const clearAwaiting = useCallback(() => setAwaiting(false), []);

  useEffect(() => {
    let unlistenDelta: (() => void) | null = null;
    let unlistenTool: (() => void) | null = null;
    let unlistenToolEnd: (() => void) | null = null;
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
        clearAwaiting();
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
        clearAwaiting();
      }),
      (fn) => {
        unlistenTool = fn;
      },
    );

    attach(
      onToolCallEnd((id, ok, view) => {
        // Resolve the badge with the matching id. We look up by id
        // (not "most recent running") so concurrent tool calls — or
        // tools that don't produce a node — still resolve correctly.
        setEntries((prev) =>
          prev.map((e) =>
            e.kind === "tool" && e.id === id
              ? { ...e, status: ok ? "ok" : "error", view }
              : e,
          ),
        );
      }),
      (fn) => {
        unlistenToolEnd = fn;
      },
    );

    attach(
      onNodeCreated((nodeId) => {
        setEntries((prev) => [
          ...prev,
          { kind: "node", id: nextId(), nodeId },
        ]);
      }),
      (fn) => {
        unlistenNode = fn;
      },
    );

    attach(
      onAgentDone(() => {
        const text = currentRef.current;
        if (text.length > 0) {
          const chips = deriveChips(text);
          setEntries((prev) => [
            ...prev,
            {
              kind: "message",
              id: nextId(),
              role: "assistant",
              text,
              chips: chips.length > 0 ? chips : undefined,
            },
          ]);
        }
        currentRef.current = "";
        setCurrent("");
        clearAwaiting();
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
        clearAwaiting();
      }),
      (fn) => {
        unlistenPlan = fn;
      },
    );

    return () => {
      cancelled = true;
      unlistenDelta?.();
      unlistenTool?.();
      unlistenToolEnd?.();
      unlistenNode?.();
      unlistenDone?.();
      unlistenPlan?.();
    };
  }, [clearAwaiting]);

  const pushUserMessage = useCallback((text: string) => {
    setEntries((prev) => [
      ...prev,
      { kind: "message", id: nextId(), role: "user", text },
    ]);
    // The user just submitted; the agent is now thinking. Cleared by
    // any downstream event.
    setAwaiting(true);
  }, []);

  const reset = useCallback(() => {
    setEntries([]);
    setCurrent("");
    currentRef.current = "";
    setPendingPlan(null);
    setAwaiting(false);
  }, []);

  const approvePlan = useCallback(
    async (steps?: Array<{ step: number; tool: string; description: string }>) => {
      const descriptions = steps?.map((s) => s.description);
      await bridgeApprovePlan(descriptions);
      setPendingPlan(null);
    },
    [],
  );

  return {
    entries,
    current,
    awaiting,
    pushUserMessage,
    reset,
    pendingPlan,
    approvePlan,
  };
}
