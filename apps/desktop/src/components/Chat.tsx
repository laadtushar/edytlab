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
  chatgptSignIn,
  chatgptTakePendingMessage,
  sendMessage as bridgeSendMessage,
  getActiveModel,
  getActiveProvider,
  getPlanFirst,
  setActiveModel,
  setPlanFirst as setPlanFirstBridge,
} from "../lib/tauri-bridge";
import type { Marker, ProviderId } from "../lib/tauri-bridge";
import {
  describeChatError,
  needsSettings,
  parseModelUnavailable,
  parsePlanUsageError,
  type PlanUsageFailure,
} from "../lib/chat-errors";
import {
  RESUME_CARRIES_ON,
  RESUME_NOTHING_KEPT,
  RESUME_RESENDS,
} from "../lib/chatgptCopy";
import { modelStorageKey } from "../lib/model-storage";
import { nothingCanRun, type PlanStep } from "../lib/planSteps";

import {
  useAgentStream,
  type LogEntry,
  type MessageEntry,
  type NodeDividerEntry,
  type NoticeEntry,
  type PlanEntry,
  type ToolEntry,
} from "../hooks/useAgentStream";

import { CapabilitiesMenu } from "./CapabilitiesMenu";
import { COMMANDS } from "./CommandPalette";
import { MessageBubble } from "./MessageBubble";
import { PlanIndicator } from "./PlanIndicator";
import { PlanStepText } from "./PlanStepText";
import {
  PlanErrorCard,
  SessionEndedCard,
  UsageLimitCard,
  type ContinueState,
} from "./PlanUsageCards";
import { AuditionPlayer } from "./AuditionPlayer";
import { SpectrumChart } from "./SpectrumChart";
import { ThinkingIndicator } from "./ThinkingIndicator";
import { ToolBadge, toolLabel } from "./ToolBadge";
import { SPEECH_SETUP } from "../lib/speechSetup";
import type { SpeechProblem } from "../lib/tauri-bridge";

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
  /**
   * Open the settings panel. Offered on the error banner when the
   * failure is "no agent configured" (#250) — that message is
   * unactionable from here otherwise, and the most common way to reach
   * it is a provider switch that left the app without a key — and when
   * the provider refused the key (#414). See `lib/chat-errors`.
   */
  onOpenSettings?: (reveal?: "speech-model") => void;
  /**
   * Changes when the active provider may have changed (Settings closed, a
   * provider chosen), so the "Using ChatGPT plan" line above the composer
   * reads it again. The line also follows `chatgpt://status` on its own.
   */
  providerEpoch?: number;
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
function isNotice(e: LogEntry): e is NoticeEntry {
  return e.kind === "notice";
}

/**
 * "1 step", "2 steps". A held edit (#415) is usually a single call, and
 * "1 steps" reads as a bug.
 */
function stepCount(n: number): string {
  return `${n} step${n === 1 ? "" : "s"}`;
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
  onOpenSettings,
  providerEpoch,
}: ChatProps, ref: React.Ref<ChatHandle>) {
  const {
    entries,
    current,
    awaiting,
    stopAwaiting,
    startAwaiting,
    pushUserMessage,
    pendingPlan,
    approvePlan,
    discardPlan,
  } = useAgentStream();
  const [input, setInput] = useState("");
  const [busy, setBusy] = useState(false);
  const [localError, setLocalError] = useState<string | null>(null);
  // Set when a turn failed on the ChatGPT plan (#474): a card of its own, told
  // apart by the failure's code. Never a refused key, and never retried by
  // itself.
  const [planFailure, setPlanFailure] = useState<PlanUsageFailure | null>(null);
  // Where the session-ended card's Continue has got to (G9).
  const [resume, setResume] = useState<ContinueState>({ kind: "idle" });
  // What a resumed turn says while it runs: the assistant carries on, or the
  // message is sent again.
  const [resumeNote, setResumeNote] = useState<string | null>(null);
  // Set when the failure is a model the provider no longer has (#468).
  // `switchTo` is what the banner offers as one explicit click, and is
  // there only if the model that failed is the one chosen in Settings: an
  // agent profile can pin its own, and switching the global one would then
  // change nothing it sends.
  const [modelGone, setModelGone] = useState<{
    defaultModel: string;
    switchTo: ProviderId | null;
  } | null>(null);
  // Which failure a lookup is about, so a slow answer for an earlier one
  // cannot put a button on a later banner.
  const failureSeq = useRef(0);
  // Inline plan step editing state
  const [editingStepIdx, setEditingStepIdx] = useState<number | null>(null);
  const [editDraft, setEditDraft] = useState("");
  const [localPlanSteps, setLocalPlanSteps] = useState<PlanStep[] | null>(null);
  const [capsOpen, setCapsOpen] = useState(false);
  // Whether to see a plan before the agent touches anything. The gate
  // itself has existed since M27 but was reachable only when a
  // classifier happened to call the request a mashup, which from the
  // outside looked arbitrary.
  const [planFirst, setPlanFirst] = useState(false);
  // Set on the first click. The preference is kept in the app's settings
  // file and read back once, here, and a click can land while that read
  // is in flight: the read then answers with what was stored *before* the
  // click, and applying it put the toggle back while the backend kept the
  // click's value — the composer promising one behaviour and the next
  // turn doing the other. The same race Settings closed for the provider
  // in #324.
  const planPickedRef = useRef(false);

  useEffect(() => {
    let cancelled = false;
    void getPlanFirst()
      .then((on) => {
        if (!cancelled && !planPickedRef.current) setPlanFirst(on);
      })
      // A composer that cannot read the preference should still work; the
      // toggle simply starts off.
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, []);

  const togglePlanFirst = useCallback(async () => {
    const next = !planFirst;
    planPickedRef.current = true;
    setPlanFirst(next); // optimistic: the button should not lag the click
    try {
      await setPlanFirstBridge(next);
    } catch (err) {
      // An error does not say the click was refused: a choice that could
      // not be saved to the settings file still holds until the app quits
      // (#569). So ask what is in force rather than assume, and put the
      // toggle there instead of lying about the state. The message says the
      // next launch is in doubt.
      let inForce = !next;
      try {
        inForce = await getPlanFirst();
      } catch {
        // Unreadable too: the click is taken as refused.
      }
      setPlanFirst(inForce);
      setLocalError(String(err));
    }
  }, [planFirst]);
  const [slashOpen, setSlashOpen] = useState(false);
  const [slashIdx, setSlashIdx] = useState(0);
  const scrollerRef = useRef<HTMLDivElement>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const lastSentRef = useRef<string>("");
  // The tools the last send switched off, so Retry sends with the same ones.
  const lastDisabledRef = useRef<string[]>([]);

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
      // The message is already in the log from the first send: one thing
      // the person typed is one bubble (#736), so Retry takes the resumed
      // path (no new bubble) with the tools that send used.
      void sendInternal(lastSentRef.current, {
        disabledTools: lastDisabledRef.current,
        note: null,
      });
    }
  };

  // After a model the provider no longer has: find out whether the model
  // that failed is the one chosen, and if so offer the provider's default.
  const offerDefaultModel = async (raw: string, seq: number) => {
    const gone = parseModelUnavailable(raw);
    if (!gone) return;
    setModelGone({ defaultModel: gone.defaultModel, switchTo: null });
    if (gone.model === gone.defaultModel) return;
    try {
      const provider = (await getActiveProvider()) as ProviderId;
      const chosen = await getActiveModel(provider);
      if (seq === failureSeq.current && chosen === gone.model) {
        setModelGone({ defaultModel: gone.defaultModel, switchTo: provider });
      }
    } catch {
      // No button, then: Open Settings is still there.
    }
  };

  const useDefaultModel = async () => {
    if (!modelGone?.switchTo) return;
    const { switchTo: provider, defaultModel } = modelGone;
    try {
      await setActiveModel(provider, defaultModel);
    } catch (err) {
      setLocalError(`could not select ${defaultModel}: ${String(err)}`);
      return;
    }
    // What Settings' model field shows, so the two agree.
    try {
      window.localStorage.setItem(modelStorageKey(provider), defaultModel);
    } catch {
      // Private mode or blocked storage: the backend has it, which is what
      // the next message uses.
    }
    setModelGone(null);
    setLocalError(`Now using ${defaultModel}. Retry to send your message again.`);
  };

  const sendInternal = async (
    wireText: string,
    // `resumed`: the message is already in the log, and was sent before the
    // session ended; send what the backend kept for it (G9), with the tools
    // that were switched off for that turn.
    resumed?: { disabledTools: string[]; note: string | null },
  ) => {
    lastSentRef.current = wireText;
    failureSeq.current += 1;
    setLocalError(null);
    setModelGone(null);
    setPlanFailure(null);
    setResume({ kind: "idle" });
    setResumeNote(resumed?.note ?? null);
    if (resumed) {
      startAwaiting();
    } else {
      pushUserMessage(wireText);
    }
    setBusy(true);
    try {
      let disabledTools: string[] = [];
      if (resumed) {
        disabledTools = resumed.disabledTools;
      } else {
        try {
          const raw = localStorage.getItem("edytlab.capabilities.disabled");
          disabledTools = raw ? (JSON.parse(raw) as string[]) : [];
        } catch {
          // corrupt localStorage entry — send with no restrictions
        }
      }
      lastDisabledRef.current = disabledTools;
      await bridgeSendMessage(wireText, disabledTools);
    } catch (err) {
      // The request failed before any agent event could clear "thinking".
      stopAwaiting();
      setResumeNote(null);
      // A failure on the ChatGPT plan has a card, picked by its code. It
      // is not an API key problem and has no banner of its own.
      const plan = parsePlanUsageError(err);
      if (plan) {
        setPlanFailure(plan);
      } else {
        setLocalError(describeChatError(err));
        void offerDefaultModel(
          String(err instanceof Error ? err.message : (err ?? "")),
          failureSeq.current,
        );
      }
    } finally {
      setBusy(false);
    }
  };

  // The session-ended card's Continue (G9): sign in, take the message the
  // backend kept, and send what it says to send. The agent is not rebuilt by
  // a sign-in, so its conversation is still there.
  const continueWithChatGpt = async () => {
    if (busy) return;
    setResume({ kind: "signing_in" });
    let pending: Awaited<ReturnType<typeof chatgptTakePendingMessage>>;
    try {
      const status = await chatgptSignIn("sign_in");
      if (status.state !== "signed_in") {
        // Cancelled, or ended some other way: the message stays kept.
        setResume({ kind: "cancelled" });
        return;
      }
      pending = await chatgptTakePendingMessage();
    } catch (err) {
      setResume({
        kind: "error",
        message: String(err instanceof Error ? err.message : (err ?? "")),
      });
      return;
    }
    if (!pending) {
      // Signed in, but nothing was kept (the app was restarted, say).
      setPlanFailure(null);
      setResume({ kind: "idle" });
      setResumeNote(RESUME_NOTHING_KEPT);
      return;
    }
    await sendInternal(pending.resumeText, {
      disabledTools: pending.disabledTools,
      note: pending.progressed ? RESUME_CARRIES_ON : RESUME_RESENDS,
    });
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
            return (
              <div key={entry.id} className="flex flex-col gap-2">
                <MessageBubble
                  role={entry.role}
                  text={entry.text}
                  chips={isLastAssistant ? entry.chips : undefined}
                  onChipClick={isLastAssistant ? submitChip : undefined}
                />
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
                {/* A view belongs to the call that produced it, not to
                    whatever the assistant happened to say next. */}
                {entry.view?.type === "spectrum" ? (
                  <SpectrumChart
                    points={entry.view.points}
                    caption={entry.view.summary ?? undefined}
                  />
                ) : null}
                {entry.view?.type === "speech_setup" ? (
                  <SpeechSetupCard
                    problem={entry.view.problem}
                    onOpenSettings={onOpenSettings}
                  />
                ) : null}
                {entry.view?.type === "audition" ? (
                  <AuditionPlayer
                    path={entry.view.path}
                    kind={entry.view.kind}
                    target={
                      entry.view.target ??
                      (entry.view.track != null
                        ? `track ${entry.view.track}`
                        : "the session")
                    }
                    startSec={entry.view.start_sec}
                    endSec={entry.view.end_sec}
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
                className="app-fade-in flex items-center gap-2 py-1 text-xs"
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
                // The card leaves at once on Run / Discard (a test holds
                // it to that), so this arriving is the motion.
                className="
                  app-fade-in
                  rounded-md border border-[var(--border)]
                  bg-[var(--surface-elev)]
                  px-3 py-2 text-xs text-[var(--text-dim)]
                "
              >
                <span className="font-medium text-[var(--text)]">Plan</span>
                <span className="ml-1.5 font-mono text-[10px] text-[var(--text-faint)]">
                  ({stepCount(entry.steps.length)})
                </span>
                <ol className="mt-1.5 space-y-0.5 pl-4 list-decimal">
                  {entry.steps.map((s, idx) => (
                    // Keyed by place, not by `step`: the model's numbers
                    // can repeat or be missing.
                    <li key={idx}>
                      <PlanStepText step={s} />
                    </li>
                  ))}
                </ol>
              </div>
            );
          }
          if (isNotice(entry)) {
            // Amber, not red: nothing failed for the user. No plan
            // arrived, and the point is that they find that out from the
            // transcript rather than by noticing its absence (#267). With
            // Plan first on the notice goes on to say the first edit will
            // be shown for approval (#415); otherwise it says the turn is
            // continuing without a checkpoint.
            return (
              <div
                key={entry.id}
                data-testid="chat-notice"
                className="
                  banner-in
                  rounded-md border border-[var(--warning-border)]
                  bg-[var(--warning-soft)]
                  px-3 py-2 text-xs text-[var(--text-dim)]
                "
              >
                {entry.text}
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

        {resumeNote ? (
          <div
            data-testid="chat-resume-note"
            role="status"
            className="
              banner-in
              rounded-md border border-[var(--warning-border)]
              bg-[var(--warning-soft)]
              px-3 py-2 text-xs text-[var(--text-dim)]
            "
          >
            {resumeNote}
          </div>
        ) : null}

        {planFailure?.code === "usage_limit" ? <UsageLimitCard failure={planFailure} /> : null}
        {planFailure?.code === "session_ended" ? (
          <SessionEndedCard
            failure={planFailure}
            resume={resume}
            busy={busy}
            onContinue={() => void continueWithChatGpt()}
          />
        ) : null}
        {planFailure && planFailure.code !== "usage_limit" && planFailure.code !== "session_ended" ? (
          <PlanErrorCard
            failure={planFailure}
            onRetry={lastSentRef.current && !busy ? retryLast : undefined}
            onOpenSettings={onOpenSettings ? () => onOpenSettings() : undefined}
          />
        ) : null}

        {localError ? (
          <div
            role="alert"
            data-testid="chat-error"
            className="
              banner-in
              rounded-md
              border border-[var(--danger-border)]
              bg-[var(--danger-soft)]
              px-3 py-2 text-xs text-[var(--danger)]
            "
          >
            <div className="flex items-start justify-between gap-2">
              <span>{localError}</span>
              {onOpenSettings && (needsSettings(localError) || modelGone) ? (
                <button
                  type="button"
                  onClick={() => onOpenSettings()}
                  data-testid="chat-error-open-settings"
                  className="
                    btn-danger shrink-0 rounded
                    px-2 py-0.5 font-mono text-[10px] uppercase tracking-wider
                  "
                >
                  Open Settings
                </button>
              ) : null}
              {modelGone?.switchTo ? (
                <button
                  type="button"
                  onClick={() => void useDefaultModel()}
                  data-testid="chat-error-use-default"
                  className="
                    btn-danger shrink-0 rounded
                    px-2 py-0.5 font-mono text-[10px] tracking-wider
                  "
                >
                  Use {modelGone.defaultModel}
                </button>
              ) : null}
              {lastSentRef.current ? (
                <button
                  type="button"
                  onClick={retryLast}
                  className="
                    btn-danger shrink-0 rounded
                    px-2 py-0.5 font-mono text-[10px] uppercase tracking-wider
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
            plan-in
            mx-3 mb-3 overflow-hidden
            rounded-lg border border-[var(--accent-border)]
            bg-[linear-gradient(180deg,var(--accent-soft),transparent)]
          "
        >
          <div className="flex items-center justify-between border-b border-[var(--accent)]/15 px-3 py-2">
            <div className="flex items-center gap-2">
              <span className="h-1.5 w-1.5 rounded-full bg-[var(--accent)] shadow-[0_0_8px_var(--accent-glow)]" />
              <span className="font-medium text-sm text-[var(--text)]">
                Plan
              </span>
              <span className="font-mono text-[10px] uppercase tracking-[0.18em] text-[var(--text-dim)]">
                {stepCount(pendingPlan.steps.length)}
              </span>
            </div>
            <div className="flex gap-1.5">
              <button
                type="button"
                data-testid="plan-discard-button"
                className="
                  btn-secondary rounded-md
                  px-2.5 py-0.5
                  text-xs
                  hover:border-[var(--danger-border)] hover:bg-[var(--danger-soft)] hover:text-[var(--danger)]
                "
                onClick={() => {
                  // The card comes down either way, but a failed reject
                  // means the backend gate may still be parked — the
                  // agent then looks stuck until its five-minute
                  // timeout, which is worth saying (#251).
                  setLocalError(null);
                  setModelGone(null);
                  void discardPlan().catch((err: unknown) =>
                    setLocalError(
                      `Discarded the plan, but the agent may still be waiting: ${String(err)}`,
                    ),
                  );
                }}
              >
                Discard
              </button>
              <button
                type="button"
                data-testid="plan-run-button"
                // A plan of steps that can only fail is not one to run
                // (#482). Discard is how the user leaves it.
                disabled={nothingCanRun(localPlanSteps ?? pendingPlan.steps)}
                className="btn-primary px-2.5 py-0.5 text-xs"
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
          {nothingCanRun(localPlanSteps ?? pendingPlan.steps) ? (
            <p
              data-testid="plan-nothing-runs"
              className="px-3 pt-2 text-xs text-[var(--danger)]"
            >
              Nothing in this plan can run in this build, so there is nothing to
              approve. Discard it.
            </p>
          ) : null}
          <ol className="space-y-1 px-3 py-2.5 pl-7 list-decimal text-xs text-[var(--text-dim)]">
            {(localPlanSteps ?? pendingPlan.steps).map((s, idx) => (
              <li
                // Keyed by place, not by `step`: the model's numbers can
                // repeat or be missing.
                key={idx}
                data-testid="plan-step"
                data-unavailable={s.unavailable ? "true" : undefined}
                // The steps arrive in reading order after the card; the
                // delay is capped in the stylesheet. Keyed by place, so
                // editing a step does not replay its entrance.
                className="stagger-in"
                style={{ "--i": idx } as React.CSSProperties}
              >
                {editingStepIdx === idx ? (
                  <span className="flex flex-col gap-1 mt-0.5">
                    <span className="font-mono text-[var(--text)]">
                      {s.tool ? toolLabel(s.tool) : "Step"}
                    </span>
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
                      className="field w-full resize-none px-2 py-1 text-xs outline-none"
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
                          rounded border border-[var(--accent-border)]
                          px-2 py-0.5 text-[10px] font-medium text-[var(--accent-text)]
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
                      <PlanStepText step={s} />
                    </span>
                    {/* A step that can only fail has nothing worth rewording. */}
                    {s.unavailable ? null : (
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
                    )}
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
              rounded-md border border-[var(--accent-border)]
              bg-[var(--accent-soft)]
              px-2.5 py-1.5
              font-mono text-[10px] uppercase tracking-wider
            "
          >
            <span className="flex items-center gap-1.5 text-[var(--accent-text)]">
              <span className="h-1.5 w-1.5 rounded-full bg-[var(--accent)] shadow-[0_0_6px_var(--accent-glow)]" />
              applies to {fmtTime(selection.start)} → {fmtTime(selection.end)}
            </span>
            <button
              type="button"
              onClick={onClearSelection}
              aria-label="Clear selection"
              className="
                rounded p-0.5 text-[var(--accent-text)]
                hover:bg-[var(--accent)]/20
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
        <PlanIndicator refreshKey={providerEpoch} />
        {slashOpen && slashFiltered.length > 0 && (
          <div className="mb-1 overflow-hidden rounded-lg border border-[var(--border-strong)] bg-[var(--surface-elev)] shadow-(--shadow-pop)">
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
            flex flex-col gap-2
            rounded-lg border border-[var(--border-input)]
            bg-[var(--surface)]
            p-2
            transition
            focus-within:border-[var(--accent)]
            focus-within:shadow-[0_0_0_3px_var(--accent-soft)]
          "
        >
          <CapabilitiesMenu
            open={capsOpen}
            onClose={() => setCapsOpen(false)}
          />
          <div data-testid="chat-toggles" className="flex items-center gap-2">
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
            className="btn-tool flex shrink-0 items-center gap-1.5 rounded-md px-2 py-1.5 font-mono text-[10px] uppercase tracking-[0.14em] transition"
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
              btn-tool inline-flex h-8 shrink-0 items-center gap-1.5
              rounded-md
              px-2
              transition
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
          </div>
          <div className="flex items-end gap-2">
          <textarea
            ref={textareaRef}
            aria-label="Message"
            placeholder="Ask for an edit… or / for commands"
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
              max-h-28 min-h-[28px] min-w-0 flex-1
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
              btn-primary inline-flex h-8 w-8 shrink-0 items-center justify-center
              transition
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
            className="btn-tool text-xs rounded px-2 py-1 transition disabled:cursor-not-allowed disabled:opacity-60"
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
            btn-tool inline-flex items-center gap-1.5
            rounded-md
            px-2.5 py-1
            font-mono text-[10px] uppercase tracking-wider
            transition
            disabled:cursor-not-allowed disabled:opacity-60
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
              className="pill ring-hover bg-[var(--surface-elev)] px-2.5 py-1 text-xs text-[var(--text-dim)] transition hover:bg-[var(--accent-soft)] hover:text-[var(--accent-text)]"
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

/**
 * What to say when a transcription cannot run because of something the user
 * sets up (#384).
 *
 * It is drawn from the failed call's own view (`speech_setup`, which the
 * backend makes from the marker that begins `transcribe`'s error), under that
 * call's badge. It used to be found by searching the assistant's reply for the
 * words of the tool's error, which the assistant rephrases, and which a
 * reply about anything else could contain.
 *
 * Each problem says what the user does. The two that Settings fixes get a
 * button to it; Settings opens on Account, where the speech model's row is.
 */
function SpeechSetupCard({
  problem,
  onOpenSettings,
}: {
  problem: SpeechProblem;
  onOpenSettings?: (reveal: "speech-model") => void;
}) {
  const text = SPEECH_SETUP[problem];
  return (
    <div
      className="rounded-md border border-[var(--warning-border)] bg-[var(--warning-soft)] px-3 py-2.5 text-xs"
      data-testid="speech-setup-card"
      data-problem={problem}
    >
      <p className="mb-1.5 font-medium text-[var(--warning)]">{text.title}</p>
      <p className="text-[var(--text-dim)]">{text.body}</p>
      {text.inSettings && onOpenSettings ? (
        <button
          type="button"
          data-testid="speech-setup-open-settings"
          onClick={() => onOpenSettings("speech-model")}
          className="
            btn-secondary mt-2 rounded
            px-2 py-0.5 font-mono text-[10px] uppercase tracking-wider
          "
        >
          Open Settings
        </button>
      ) : null}
    </div>
  );
}

function fmtTime(sec: number): string {
  const m = Math.floor(sec / 60);
  const s = sec - m * 60;
  return `${m}:${s.toFixed(2).padStart(5, "0")}`;
}
