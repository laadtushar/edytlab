/**
 * Chat — right-pane conversation panel (Studio Onyx).
 *
 * Renders the agent transcript: alternating message bubbles, tool
 * badges, and "new node" dividers. Plan-approval cards appear pinned
 * just above the input row. The input itself is a single-line
 * textarea that grows up to 4 lines, with `Enter` to submit and
 * `Shift+Enter` for a newline — feels more like a chat app than a
 * form.
 *
 * All Rust calls go through `tauri-bridge`; bridge errors are rendered
 * as inline assistant turns so the user sees a recoverable message
 * instead of a console trace.
 */

import React, { forwardRef, useCallback, useEffect, useImperativeHandle, useMemo, useRef, useState } from "react";

import {
  sendMessage as bridgeSendMessage,
  rejectPlan,
  getPlanFirst,
  setPlanFirst as setPlanFirstBridge,
} from "../lib/tauri-bridge";
import type { Marker } from "../lib/tauri-bridge";

import {
  useAgentStream,
  type LogEntry,
  type MessageEntry,
  type NodeDividerEntry,
  type PlanEntry,
  type ToolEntry,
} from "../hooks/useAgentStream";

import { CapabilitiesMenu } from "./CapabilitiesMenu";
import { COMMANDS } from "./CommandPalette";
import { MessageBubble } from "./MessageBubble";
import { SpectrumChart } from "./SpectrumChart";
import { ThinkingIndicator } from "./ThinkingIndicator";
import { ToolBadge } from "./ToolBadge";

export interface ChatProps {
  rendering?: boolean;
  onRequestRenderPreview?: () => void;
  /**
   * Active timeline selection (in seconds), if any. When set, the
   * input shows a pill above it and outgoing messages are prefixed
   * with `[apply to MM:SS.ms-MM:SS.ms]` so the agent picks up the
   * region constraint without a separate state channel.
   */
  selection?: { start: number; end: number } | null;
  /** Called when the user clears the selection from the chat pill. */
  onClearSelection?: () => void;
  /** Current markers — point markers are included in the outgoing message prefix. */
  markers?: Marker[];
  /** Called when the user clicks the Export Selection button in the header. */
  onExportSelection?: () => void;
  /** Whether an export is currently in progress (disables the button). */
  exporting?: boolean;
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
function isPlan(e: LogEntry): e is PlanEntry {
  return e.kind === "plan";
}

const CHAT_HINTS = [
  "make this 6 dB louder",
  "fade out the last 3 seconds",
  "remove background noise",
  "what tools do you have?",
];

export interface ChatHandle {
  fillInput(text: string): void;
}

export const Chat = forwardRef<ChatHandle, ChatProps>(function Chat({
  rendering,
  onRequestRenderPreview,
  selection,
  onClearSelection,
  markers,
  onExportSelection,
  exporting,
}: ChatProps, ref: React.Ref<ChatHandle>) {
  const {
    entries,
    current,
    awaiting,
    pushUserMessage,
    pendingPlan,
    approvePlan,
  } = useAgentStream();
  const [input, setInput] = useState("");
  const [busy, setBusy] = useState(false);
  const [localError, setLocalError] = useState<string | null>(null);
  // Inline plan step editing state
  const [editingStepIdx, setEditingStepIdx] = useState<number | null>(null);
  const [editDraft, setEditDraft] = useState("");
  const [localPlanSteps, setLocalPlanSteps] = useState<Array<{ step: number; tool: string; description: string }> | null>(null);
  const [capsOpen, setCapsOpen] = useState(false);
  // Whether to see a plan before the agent touches anything. The gate
  // itself has existed since M27 but was reachable only when a
  // classifier happened to call the request a mashup, which from the
  // outside looked arbitrary.
  const [planFirst, setPlanFirst] = useState(false);

  useEffect(() => {
    let cancelled = false;
    void getPlanFirst()
      .then((on) => {
        if (!cancelled) setPlanFirst(on);
      })
      // A composer that cannot read one optional preference should still
      // work; the toggle simply starts off.
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, []);

  const togglePlanFirst = useCallback(async () => {
    const next = !planFirst;
    setPlanFirst(next); // optimistic: the button should not lag the click
    try {
      await setPlanFirstBridge(next);
    } catch {
      setPlanFirst(!next); // put it back rather than lie about the state
    }
  }, [planFirst]);
  const [slashOpen, setSlashOpen] = useState(false);
  const [slashIdx, setSlashIdx] = useState(0);
  const scrollerRef = useRef<HTMLDivElement>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const lastSentRef = useRef<string>("");

  const slashQuery = slashOpen ? input.slice(1).toLowerCase() : "";
  const slashFiltered = useMemo(() => {
    if (!slashOpen) return [];
    const q = slashQuery.trim();
    if (!q) return COMMANDS.slice(0, 8);
    return COMMANDS.filter(
      (c) =>
        c.label.toLowerCase().includes(q) ||
        c.category.toLowerCase().includes(q) ||
        (c.tags ?? []).some((t) => t.includes(q)),
    ).slice(0, 8);
  }, [slashOpen, slashQuery]);

  const selectSlash = useCallback((cmd: (typeof COMMANDS)[0]) => {
    setInput(cmd.prompt);
    setSlashOpen(false);
    setSlashIdx(0);
    setTimeout(() => textareaRef.current?.focus(), 0);
  }, []);

  useImperativeHandle(ref, () => ({
    fillInput(text: string) {
      setInput(text);
      setTimeout(() => textareaRef.current?.focus(), 0);
    },
  }));

  // Sync local plan steps when a new pending plan arrives.
  useEffect(() => {
    if (pendingPlan) {
      setLocalPlanSteps(pendingPlan.steps.map((s) => ({ ...s })));
      setEditingStepIdx(null);
      setEditDraft("");
    } else {
      setLocalPlanSteps(null);
    }
  }, [pendingPlan]);

  // Auto-scroll to bottom on new entries / streaming deltas / awaiting state.
  useEffect(() => {
    const el = scrollerRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [entries.length, current, awaiting]);

  // Auto-grow the textarea up to ~4 lines.
  useEffect(() => {
    const ta = textareaRef.current;
    if (!ta) return;
    ta.style.height = "0";
    ta.style.height = `${Math.min(ta.scrollHeight, 112)}px`;
  }, [input]);

  // Single send path used by both the form submit and chip clicks.
  // Whoever calls this is responsible for the `busy` short-circuit and
  // for shaping the wire text (selection / marker prefix or not).
  const retryLast = () => {
    if (lastSentRef.current && !busy) {
      void sendInternal(lastSentRef.current);
    }
  };

  const sendInternal = async (wireText: string) => {
    lastSentRef.current = wireText;
    setLocalError(null);
    pushUserMessage(wireText);
    setBusy(true);
    try {
      let disabledTools: string[] = [];
      try {
        const raw = localStorage.getItem("edytlab.capabilities.disabled");
        disabledTools = raw ? (JSON.parse(raw) as string[]) : [];
      } catch {
        // corrupt localStorage entry — send with no restrictions
      }
      await bridgeSendMessage(wireText, disabledTools);
    } catch (err) {
      setLocalError(friendlyError(err));
    } finally {
      setBusy(false);
    }
  };

  const submit = async () => {
    const text = input.trim();
    if (!text || busy) return;
    setInput("");
    const markerCtx =
      markers && markers.length > 0
        ? `[markers: ${markers
            .filter((m): m is typeof m & { kind: "marker"; time_sec: number } => m.kind === "marker")
            .map((m) => `${m.name}@${fmtTime(m.time_sec)}`)
            .join(", ")}] `
        : "";
    const wireText = selection
      ? `[apply to ${fmtTime(selection.start)}-${fmtTime(selection.end)}] ${markerCtx}${text}`
      : markerCtx + text;
    await sendInternal(wireText);
  };

  // Send a chip-suggested prompt as if the user had typed it. Selection
  // / marker context is intentionally NOT prefixed: chips represent the
  // user accepting the agent's literal suggestion, not a new
  // contextual request.
  const submitChip = async (prompt: string) => {
    if (busy) return;
    await sendInternal(prompt);
  };

  const isEmpty = entries.length === 0 && current.length === 0;

  // Chips only show on the *most recent* assistant message — older
  // turns become plain transcript so the eye doesn't bounce between
  // multiple chip rows. Memoised because the render loop fires on every
  // streaming delta and we don't want to walk the transcript each tick.
  const lastAssistantIdx = useMemo(() => {
    for (let i = entries.length - 1; i >= 0; i -= 1) {
      const e = entries[i];
      if (isMessage(e) && e.role === "assistant") return i;
    }
    return -1;
  }, [entries]);

  return (
    <div
      data-testid="chat-root"
      className="flex h-full w-full flex-col bg-[var(--surface)]"
    >
      <ChatHeader
        rendering={rendering}
        onRequestRenderPreview={onRequestRenderPreview}
        selection={selection}
        onExportSelection={onExportSelection}
        exporting={exporting}
      />

      <div
        ref={scrollerRef}
        data-testid="chat-scroller"
        className="
          flex-1 space-y-3 overflow-y-auto
          px-4 py-4
        "
      >
        {isEmpty ? <ChatEmptyHints onPick={(t) => setInput(t)} /> : null}

        {entries.map((entry, idx) => {
          if (isMessage(entry)) {
            const isLastAssistant =
              entry.role === "assistant" && idx === lastAssistantIdx;
            const showWhisperSetup =
              entry.role === "assistant" && isWhisperError(entry.text);
            return (
              <div key={entry.id} className="flex flex-col gap-2">
                <MessageBubble
                  role={entry.role}
                  text={entry.text}
                  chips={isLastAssistant ? entry.chips : undefined}
                  onChipClick={isLastAssistant ? submitChip : undefined}
                />
                {showWhisperSetup ? <WhisperSetupCard /> : null}
              </div>
            );
          }
          if (isTool(entry)) {
            return (
              <div key={entry.id} className="flex flex-col items-start gap-1.5">
                <ToolBadge
                  name={entry.name}
                  status={entry.status}
                  result={entry.result}
                />
                {/* A chart belongs to the call that computed it, not to
                    whatever the assistant happened to say next. */}
                {entry.view?.type === "spectrum" ? (
                  <SpectrumChart
                    points={entry.view.points}
                    caption={entry.view.summary ?? undefined}
                  />
                ) : null}
              </div>
            );
          }
          if (isNode(entry)) {
            return (
              <div
                key={entry.id}
                data-testid="node-divider"
                className="flex items-center gap-2 py-1 text-xs"
              >
                <div className="h-px flex-1 bg-[var(--border)]" />
                <span className="font-mono text-[10px] uppercase tracking-[0.18em] text-[var(--text-faint)]">
                  node {entry.nodeId.slice(0, 7)}
                </span>
                <div className="h-px flex-1 bg-[var(--border)]" />
              </div>
            );
          }
          if (isPlan(entry)) {
            return (
              <div
                key={entry.id}
                data-testid="plan-history-entry"
                className="
                  rounded-md border border-[var(--border)]
                  bg-[var(--surface-elev)]
                  px-3 py-2 text-xs text-[var(--text-dim)]
                "
              >
                <span className="font-medium text-[var(--text)]">
                  Mashup Plan
                </span>
                <span className="ml-1.5 font-mono text-[10px] text-[var(--text-faint)]">
                  ({entry.steps.length} steps)
                </span>
                <ol className="mt-1.5 space-y-0.5 pl-4 list-decimal">
                  {entry.steps.map((s) => (
                    <li key={s.step}>
                      <span className="font-mono text-[var(--text)]">
                        {s.tool}
                      </span>{" "}
                      <span className="text-[var(--text-dim)]">
                        — {s.description}
                      </span>
                    </li>
                  ))}
                </ol>
              </div>
            );
          }
          return null;
        })}

        {current.length > 0 ? (
          <MessageBubble role="assistant" text={current} pending />
        ) : null}

        {awaiting && current.length === 0 ? (
          <ThinkingIndicator awaiting={awaiting} />
        ) : null}

        {localError ? (
          <div
            role="alert"
            data-testid="chat-error"
            className="
              rounded-md
              border border-[var(--danger)]/40
              bg-[var(--danger)]/10
              px-3 py-2 text-xs text-[var(--danger)]
            "
          >
            <div className="flex items-start justify-between gap-2">
              <span>{localError}</span>
              {lastSentRef.current ? (
                <button
                  type="button"
                  onClick={retryLast}
                  className="
                    shrink-0 rounded border border-[var(--danger)]/40
                    px-2 py-0.5 font-mono text-[10px] uppercase tracking-wider
                    hover:bg-[var(--danger)]/20
                  "
                >
                  Retry
                </button>
              ) : null}
            </div>
          </div>
        ) : null}
      </div>

      {pendingPlan ? (
        <div
          data-testid="plan-approval-card"
          className="
            mx-3 mb-3 overflow-hidden
            rounded-md border border-[var(--accent)]/35
            bg-[linear-gradient(180deg,var(--accent-soft),transparent)]
          "
        >
          <div className="flex items-center justify-between border-b border-[var(--accent)]/15 px-3 py-2">
            <div className="flex items-center gap-2">
              <span className="h-1.5 w-1.5 rounded-full bg-[var(--accent)] shadow-[0_0_8px_var(--accent-glow)]" />
              <span className="font-medium text-sm text-[var(--text)]">
                Mashup Plan
              </span>
              <span className="font-mono text-[10px] uppercase tracking-[0.18em] text-[var(--text-dim)]">
                {pendingPlan.steps.length} steps
              </span>
            </div>
            <div className="flex gap-1.5">
              <button
                type="button"
                data-testid="plan-discard-button"
                className="
                  rounded border border-[var(--border-strong)]
                  px-2.5 py-0.5
                  text-xs text-[var(--text-dim)]
                  hover:border-[var(--danger)]/50 hover:text-[var(--danger)]
                "
                onClick={() => void rejectPlan()}
              >
                Discard
              </button>
              <button
                type="button"
                data-testid="plan-run-button"
                className="
                  rounded
                  bg-[var(--accent)]
                  px-2.5 py-0.5
                  text-xs font-medium text-[var(--bg)]
                  shadow-[0_4px_10px_-4px_var(--accent-glow)]
                  hover:bg-[#ffa05f]
                "
                onClick={() => {
                  // Only forward edited steps to the backend when at least
                  // one description differs from the original plan.
                  const hasEdits = localPlanSteps !== null &&
                    localPlanSteps.some(
                      (s, i) => s.description !== (pendingPlan.steps[i]?.description ?? ""),
                    );
                  void approvePlan(hasEdits ? localPlanSteps : undefined);
                }}
              >
                Run
              </button>
            </div>
          </div>
          <ol className="space-y-1 px-3 py-2.5 pl-7 list-decimal text-xs text-[var(--text-dim)]">
            {(localPlanSteps ?? pendingPlan.steps).map((s, idx) => (
              <li key={s.step}>
                {editingStepIdx === idx ? (
                  <span className="flex flex-col gap-1 mt-0.5">
                    <span className="font-mono text-[var(--text)]">{s.tool}</span>
                    <textarea
                      data-testid="plan-step-editor"
                      autoFocus
                      rows={2}
                      value={editDraft}
                      onChange={(e) => setEditDraft(e.target.value)}
                      onKeyDown={(e) => {
                        if (e.key === "Escape") {
                          setEditingStepIdx(null);
                          setEditDraft("");
                        }
                      }}
                      className="
                        w-full resize-none rounded border border-[var(--accent)]/45
                        bg-[var(--surface)] px-2 py-1
                        text-xs text-[var(--text)]
                        outline-none focus:border-[var(--accent)]/70
                      "
                    />
                    <span className="flex gap-1">
                      <button
                        type="button"
                        data-testid="plan-step-save"
                        disabled={!editDraft.trim()}
                        onClick={() => {
                          const trimmed = editDraft.trim();
                          if (trimmed) {
                            setLocalPlanSteps((prev) => {
                              if (!prev) return prev;
                              return prev.map((step, i) =>
                                i === idx ? { ...step, description: trimmed } : step,
                              );
                            });
                          }
                          setEditingStepIdx(null);
                          setEditDraft("");
                        }}
                        className="
                          rounded border border-[var(--accent)]/45
                          px-2 py-0.5 text-[10px] font-medium text-[var(--accent)]
                          hover:bg-[var(--accent-soft)]
                          disabled:opacity-40 disabled:cursor-not-allowed
                        "
                      >
                        Save
                      </button>
                      <button
                        type="button"
                        data-testid="plan-step-cancel"
                        onClick={() => {
                          setEditingStepIdx(null);
                          setEditDraft("");
                        }}
                        className="
                          rounded border border-[var(--border-strong)]
                          px-2 py-0.5 text-[10px] text-[var(--text-dim)]
                          hover:bg-[var(--surface-elev-2)]
                        "
                      >
                        Cancel
                      </button>
                    </span>
                  </span>
                ) : (
                  <span className="group flex items-start gap-1.5">
                    <span className="flex-1">
                      <span className="font-mono text-[var(--text)]">{s.tool}</span>{" "}
                      <span>— {s.description}</span>
                    </span>
                    <button
                      type="button"
                      data-testid="plan-edit-button"
                      onClick={() => {
                        setEditingStepIdx(idx);
                        setEditDraft(s.description);
                      }}
                      className="
                        shrink-0 rounded border border-[var(--border-strong)]
                        px-1.5 py-0.5 text-[10px] text-[var(--text-faint)]
                        opacity-0 group-hover:opacity-100 transition-opacity
                        hover:bg-[var(--surface-elev-2)] hover:text-[var(--text-dim)]
                      "
                    >
                      Edit
                    </button>
                  </span>
                )}
              </li>
            ))}
          </ol>
        </div>
      ) : null}

      <form
        data-testid="chat-form"
        className="
          relative border-t border-[var(--border)]
          bg-[var(--surface-elev)]
          p-3
        "
        onSubmit={(e) => {
          e.preventDefault();
          void submit();
        }}
      >
        {selection ? (
          <div
            data-testid="chat-selection-pill"
            className="
              mb-2 flex items-center justify-between gap-2
              rounded-md border border-[var(--accent)]/40
              bg-[var(--accent-soft)]
              px-2.5 py-1.5
              font-mono text-[10px] uppercase tracking-wider
            "
          >
            <span className="flex items-center gap-1.5 text-[var(--accent)]">
              <span className="h-1.5 w-1.5 rounded-full bg-[var(--accent)] shadow-[0_0_6px_var(--accent-glow)]" />
              applies to {fmtTime(selection.start)} → {fmtTime(selection.end)}
            </span>
            <button
              type="button"
              onClick={onClearSelection}
              aria-label="Clear selection"
              className="
                rounded p-0.5 text-[var(--accent)]/80
                hover:bg-[var(--accent)]/20 hover:text-[var(--accent)]
              "
            >
              <svg
                width="11"
                height="11"
                viewBox="0 0 11 11"
                fill="none"
                stroke="currentColor"
                strokeWidth="1.6"
                strokeLinecap="round"
                aria-hidden="true"
              >
                <path d="M2.5 2.5l6 6M8.5 2.5l-6 6" />
              </svg>
            </button>
          </div>
        ) : null}
        {slashOpen && slashFiltered.length > 0 && (
          <div className="mb-1 overflow-hidden rounded-lg border border-[var(--border-strong)] bg-[var(--surface-elev)] shadow-[0_8px_24px_-4px_rgba(0,0,0,0.6)]">
            <div className="px-3 py-1.5 border-b border-[var(--border)]">
              <span className="font-mono text-[9px] uppercase tracking-[0.2em] text-[var(--text-faint)]">Commands</span>
            </div>
            {slashFiltered.map((cmd, i) => (
              <button
                key={cmd.label}
                type="button"
                onMouseDown={(e) => { e.preventDefault(); selectSlash(cmd); }}
                onMouseEnter={() => setSlashIdx(i)}
                className={[
                  "flex w-full items-center gap-3 px-3 py-2 text-left transition",
                  i === slashIdx
                    ? "bg-[var(--accent-soft)] text-[var(--text)]"
                    : "text-[var(--text-dim)] hover:bg-[var(--surface-elev-2)]",
                ].join(" ")}
              >
                <span className="flex-1 min-w-0">
                  <span className="block text-sm font-medium leading-tight truncate">{cmd.label}</span>
                  <span className="block text-[10px] leading-snug text-[var(--text-faint)] truncate">{cmd.description}</span>
                </span>
                <span className="shrink-0 font-mono text-[9px] uppercase tracking-wider text-[var(--text-faint)]">{cmd.category}</span>
              </button>
            ))}
          </div>
        )}
        <div
          className="
            relative
            flex items-end gap-2
            rounded-lg border border-[var(--border-strong)]
            bg-[var(--surface)]
            p-2
            transition
            focus-within:border-[var(--accent)]/45
            focus-within:shadow-[0_0_0_3px_var(--accent-soft)]
          "
        >
          <CapabilitiesMenu
            open={capsOpen}
            onClose={() => setCapsOpen(false)}
          />
          <button
            type="button"
            data-testid="plan-first-toggle"
            aria-label="Plan before acting"
            aria-pressed={planFirst}
            title={
              planFirst
                ? "Planning first — the agent will show its steps before running them"
                : "Acting directly — turn this on to review a plan first"
            }
            onMouseDown={(e) => e.stopPropagation()}
            onClick={() => void togglePlanFirst()}
            className={
              "flex shrink-0 items-center gap-1.5 rounded-md px-2 py-1.5 " +
              "font-mono text-[10px] uppercase tracking-[0.14em] transition " +
              (planFirst
                ? "bg-[var(--accent-soft)] text-[var(--accent)] border border-[var(--accent)]/40"
                : "border border-[var(--border-strong)] text-[var(--text-faint)] hover:text-[var(--text-dim)]")
            }
          >
            Plan
          </button>
          <button
            type="button"
            data-testid="capabilities-toggle"
            aria-label="Show available tools and capabilities"
            aria-expanded={capsOpen}
            title="View & toggle agent tools"
            onMouseDown={(e) => e.stopPropagation()}
            onClick={() => setCapsOpen((v) => !v)}
            className="
              inline-flex h-8 shrink-0 items-center gap-1.5
              rounded-md
              border border-[var(--border-strong)]
              bg-[var(--surface-elev)]
              px-2
              text-[var(--text-dim)]
              transition
              hover:border-[var(--accent)]/45 hover:bg-[var(--accent-soft)] hover:text-[var(--accent)]
            "
          >
            <svg
              width="12"
              height="12"
              viewBox="0 0 12 12"
              fill="none"
              stroke="currentColor"
              strokeWidth="1.7"
              strokeLinecap="round"
              strokeLinejoin="round"
              aria-hidden="true"
            >
              <rect x="1.5" y="1.5" width="3.5" height="3.5" rx="0.6" />
              <rect x="7" y="1.5" width="3.5" height="3.5" rx="0.6" />
              <rect x="1.5" y="7" width="3.5" height="3.5" rx="0.6" />
              <rect x="7" y="7" width="3.5" height="3.5" rx="0.6" />
            </svg>
            <span className="font-mono text-[10px] uppercase tracking-wider">Tools</span>
          </button>
          <textarea
            ref={textareaRef}
            aria-label="Message"
            placeholder="Tell the agent what to edit… or type / for commands"
            disabled={busy}
            value={input}
            rows={1}
            onChange={(e) => {
              const val = e.target.value;
              setInput(val);
              if (val.startsWith("/")) {
                setSlashOpen(true);
                setSlashIdx(0);
              } else {
                setSlashOpen(false);
              }
            }}
            onKeyDown={(e) => {
              if (slashOpen) {
                if (e.key === "ArrowDown") {
                  e.preventDefault();
                  setSlashIdx((i) => Math.min(i + 1, slashFiltered.length - 1));
                  return;
                }
                if (e.key === "ArrowUp") {
                  e.preventDefault();
                  setSlashIdx((i) => Math.max(i - 1, 0));
                  return;
                }
                if (e.key === "Enter" && slashFiltered[slashIdx]) {
                  e.preventDefault();
                  selectSlash(slashFiltered[slashIdx]);
                  return;
                }
                if (e.key === "Escape") {
                  e.preventDefault();
                  setSlashOpen(false);
                  return;
                }
              }
              if (e.key === "Enter" && !e.shiftKey) {
                e.preventDefault();
                void submit();
              }
            }}
            className="
              max-h-28 min-h-[28px] flex-1
              resize-none bg-transparent
              text-sm text-[var(--text)]
              outline-none
              placeholder:text-[var(--text-faint)]
              disabled:opacity-60
            "
          />
          <button
            type="submit"
            disabled={busy || input.trim().length === 0}
            aria-label="Send message"
            className="
              inline-flex h-8 w-8 shrink-0 items-center justify-center
              rounded-md
              bg-[var(--accent)]
              text-[var(--bg)]
              shadow-[0_4px_10px_-4px_var(--accent-glow)]
              transition
              hover:bg-[#ffa05f]
              disabled:cursor-not-allowed disabled:bg-[var(--surface-elev-2)] disabled:text-[var(--text-faint)] disabled:shadow-none
            "
          >
            {busy ? (
              <span className="h-1.5 w-1.5 animate-pulse rounded-full bg-current" />
            ) : (
              <svg
                width="14"
                height="14"
                viewBox="0 0 14 14"
                fill="none"
                stroke="currentColor"
                strokeWidth="1.8"
                strokeLinecap="round"
                strokeLinejoin="round"
                aria-hidden="true"
              >
                <path d="M2 7h9M7 3l4 4-4 4" />
              </svg>
            )}
          </button>
        </div>
        <p className="mt-1.5 px-1 text-[10px] font-mono uppercase tracking-[0.18em] text-[var(--text-faint)]">
          enter to send · shift+enter for newline · / for commands · ctrl+k for all tools
        </p>
      </form>
    </div>
  );
}

);
interface ChatHeaderProps {
  rendering?: boolean;
  onRequestRenderPreview?: () => void;
  selection?: { start: number; end: number } | null;
  onExportSelection?: () => void;
  exporting?: boolean;
}

function ChatHeader({
  rendering,
  onRequestRenderPreview,
  selection,
  onExportSelection,
  exporting,
}: ChatHeaderProps) {
  return (
    <div
      className="
        flex shrink-0 items-center justify-between
        border-b border-[var(--border)]
        bg-[var(--surface-elev)]
        px-4 py-2.5
      "
    >
      <div className="flex items-center gap-2">
        <h2 className="font-mono text-[11px] uppercase tracking-[0.2em] text-[var(--text-dim)]">
          Assistant
        </h2>
      </div>
      <div className="flex items-center gap-2">
        {selection ? (
          <button
            type="button"
            data-testid="export-selection-btn"
            onClick={onExportSelection}
            disabled={!!exporting}
            className="text-xs text-amber-400 border border-amber-400/40 rounded px-2 py-1 hover:bg-amber-400/10 transition-colors disabled:cursor-not-allowed disabled:opacity-60"
          >
            Export Selection
          </button>
        ) : null}
        <button
          type="button"
          data-testid="render-preview-button"
          disabled={!!rendering}
          onClick={onRequestRenderPreview}
          className="
            inline-flex items-center gap-1.5
            rounded-md
            border border-[var(--border-strong)]
            bg-[var(--surface)]
            px-2.5 py-1
            font-mono text-[10px] uppercase tracking-wider text-[var(--text-dim)]
            transition
            hover:border-[var(--accent)]/50 hover:bg-[var(--accent-soft)] hover:text-[var(--accent)]
            disabled:cursor-not-allowed disabled:opacity-60 disabled:hover:bg-[var(--surface)] disabled:hover:text-[var(--text-dim)]
          "
        >
          <svg
            width="10"
            height="10"
            viewBox="0 0 10 10"
            fill="currentColor"
            aria-hidden="true"
          >
            <path d="M2 1.5v7l6-3.5z" />
          </svg>
          {rendering ? "rendering" : "preview"}
        </button>
      </div>
    </div>
  );
}

interface ChatEmptyHintsProps {
  onPick: (text: string) => void;
}

function ChatEmptyHints({ onPick }: ChatEmptyHintsProps) {
  return (
    <div className="flex flex-col gap-3 py-2">
      <div className="flex flex-col gap-1">
        <p className="font-mono text-[10px] uppercase tracking-[0.2em] text-[var(--text-dim)]">
          How it works
        </p>
        <p className="text-xs leading-relaxed text-[var(--text-faint)]">
          Load audio on the left, then describe what you want. The assistant uses tools to edit your session and saves each change as a node in the graph.
        </p>
      </div>

      <div>
        <p className="mb-1.5 font-mono text-[10px] uppercase tracking-[0.2em] text-[var(--text-faint)]">
          Try saying
        </p>
        <div className="flex flex-wrap gap-1.5">
          {CHAT_HINTS.map((h) => (
            <button
              key={h}
              type="button"
              onClick={() => onPick(h)}
              className="rounded-full border border-[var(--border)] bg-[var(--surface-elev)] px-2.5 py-1 text-xs text-[var(--text-dim)] transition hover:border-[var(--accent)]/45 hover:bg-[var(--accent-soft)] hover:text-[var(--accent)]"
            >
              &quot;{h}&quot;
            </button>
          ))}
        </div>
      </div>

      <p className="text-[10px] text-[var(--text-faint)]">
        Click the{" "}
        <span className="rounded border border-[var(--border-strong)] bg-[var(--surface-elev-2)] px-1 font-mono">
          Tools
        </span>{" "}
        button below to see all available audio editing operations.
      </p>
    </div>
  );
}

function isWhisperError(text: string): boolean {
  const t = text.toLowerCase();
  return (
    t.includes("whisper_model_path") ||
    t.includes("fetch-models.sh") ||
    t.includes("whisper model not found")
  );
}

function WhisperSetupCard() {
  return (
    <div className="rounded-md border border-amber-500/30 bg-amber-500/8 px-3 py-2.5 text-xs">
      <p className="mb-1.5 font-medium text-amber-400">
        Whisper model not installed
      </p>
      <ol className="list-decimal space-y-1 pl-4 text-[var(--text-dim)]">
        <li>
          Run{" "}
          <code className="rounded bg-[var(--surface-elev-2)] px-1 font-mono">
            scripts/fetch-models.sh
          </code>{" "}
          in the repo root to download the model
        </li>
        <li>
          Set{" "}
          <code className="rounded bg-[var(--surface-elev-2)] px-1 font-mono">
            WHISPER_MODEL_PATH
          </code>{" "}
          to the downloaded <code className="font-mono">.onnx</code> path
        </li>
        <li>Restart edytlab, then try transcribing again</li>
      </ol>
    </div>
  );
}

function friendlyError(err: unknown): string {
  const raw = String(
    err instanceof Error ? err.message : (err ?? "unknown error"),
  );
  return `Could not complete request: ${raw}`;
}

function fmtTime(sec: number): string {
  const m = Math.floor(sec / 60);
  const s = sec - m * 60;
  return `${m}:${s.toFixed(2).padStart(5, "0")}`;
}
