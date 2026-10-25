/**
 * The cards Chat draws for a turn that failed on the ChatGPT plan (#474 PR 6).
 *
 * A plan failure is told apart by its code (`parsePlanUsageError`), never by
 * its words, so none of these is a refused API key: the plan has no key, and
 * the chat never sends the person to Settings to paste one.
 *
 * - **Usage limit** (G5): OpenAI's own words. No automatic retry, no fallback
 *   to another provider, and no Retry button either: "Signing in again or
 *   retrying won't restore usage". **Manage usage** opens ChatGPT's usage
 *   page. The request ID is shown small, with a copy button, and a short
 *   neutral text can be copied for a workspace admin.
 * - **Session ended** (G9): the message is kept, and **Continue with ChatGPT**
 *   signs in and sends it again (the flow is Chat's).
 * - **Anything else** (`not_eligible`, `plan_disabled`, `unavailable`,
 *   `unsupported_request`, `refused`): one plain sentence and the request ID.
 */

import { useEffect, useRef, useState } from "react";

import type { PlanUsageFailure } from "../lib/chat-errors";
import {
  COPIED_LABEL,
  COPY_FAILED_LABEL,
  COPY_FOR_ADMIN_LABEL,
  COPY_REQUEST_ID_LABEL,
  CONTINUE_CANCELLED,
  CONTINUE_LABEL,
  MANAGE_USAGE_LABEL,
  PLAN_ERROR_SENTENCES,
  SESSION_ENDED_BODY,
  SESSION_ENDED_TITLE,
  SIGNING_IN,
  USAGE_LIMIT_APP_LIMIT,
  USAGE_LIMIT_NO_RETRY,
  USAGE_LIMIT_TITLE,
  adminRequestText,
} from "../lib/chatgptCopy";
import { openChatgptUsage } from "../lib/tauri-bridge";
import { ChatGptLogo } from "./ChatGptLogo";

const CARD_CLASS =
  "banner-in rounded-md border border-[var(--warning-border)] bg-[var(--warning-soft)] px-3 py-2 text-xs text-[var(--text-dim)]";
const SMALL_BUTTON =
  "btn-secondary shrink-0 rounded px-2 py-0.5 font-mono text-[10px] tracking-wider transition";

function messageOf(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

/** A button that puts `text` on the clipboard and says it did. */
function CopyButton({
  text,
  label,
  testId,
}: {
  text: string;
  label: string;
  testId: string;
}) {
  const [state, setState] = useState<"idle" | "copied" | "failed">("idle");
  const timer = useRef<number | null>(null);

  useEffect(
    () => () => {
      if (timer.current !== null) window.clearTimeout(timer.current);
    },
    [],
  );

  const copy = async () => {
    let next: "copied" | "failed" = "copied";
    try {
      await navigator.clipboard.writeText(text);
    } catch {
      // No clipboard, or it refused: say so, the text is not lost.
      next = "failed";
    }
    setState(next);
    if (timer.current !== null) window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => setState("idle"), 2000);
  };

  return (
    <button type="button" data-testid={testId} onClick={() => void copy()} className={SMALL_BUTTON}>
      {state === "copied" ? COPIED_LABEL : state === "failed" ? COPY_FAILED_LABEL : label}
    </button>
  );
}

/** OpenAI's request ID, small, with a button to copy it. Nothing when there is none. */
function RequestId({ id }: { id: string | null }) {
  if (!id) return null;
  return (
    <div className="mt-2 flex flex-wrap items-center gap-2">
      <span
        data-testid="plan-request-id"
        className="break-all font-mono text-[10px] text-[var(--text-faint)]"
      >
        Request ID: {id}
      </span>
      <CopyButton text={id} label={COPY_REQUEST_ID_LABEL} testId="plan-request-id-copy" />
    </div>
  );
}

/** The usage page, in the system browser, through the backend (G15). */
function ManageUsageButton() {
  const [problem, setProblem] = useState<string | null>(null);
  return (
    <>
      <button
        type="button"
        data-testid="plan-manage-usage"
        onClick={() => {
          setProblem(null);
          openChatgptUsage().catch((e: unknown) => setProblem(messageOf(e)));
        }}
        className="btn-primary px-3 py-1 text-xs"
      >
        {MANAGE_USAGE_LABEL}
      </button>
      {problem ? (
        <span data-testid="plan-manage-usage-error" role="alert" className="text-[var(--danger)]">
          {problem}
        </span>
      ) : null}
    </>
  );
}

export function UsageLimitCard({ failure }: { failure: PlanUsageFailure }) {
  return (
    <div role="alert" data-testid="plan-usage-limit-card" data-code="usage_limit" className={CARD_CLASS}>
      <p className="flex items-start gap-2 font-medium text-[var(--text)]">
        <ChatGptLogo tone="on-dark" size={16} className="mt-px" />
        <span>{USAGE_LIMIT_TITLE}</span>
      </p>
      <p className="mt-1">{USAGE_LIMIT_APP_LIMIT}</p>
      <p className="mt-1">{USAGE_LIMIT_NO_RETRY}</p>
      <div className="mt-2 flex flex-wrap items-center gap-2">
        <ManageUsageButton />
        <CopyButton
          text={adminRequestText(failure.requestId)}
          label={COPY_FOR_ADMIN_LABEL}
          testId="plan-copy-for-admin"
        />
      </div>
      <RequestId id={failure.requestId} />
    </div>
  );
}

/** What the Continue flow is doing, for the session-ended card. */
export type ContinueState =
  | { kind: "idle" }
  | { kind: "signing_in" }
  | { kind: "cancelled" }
  | { kind: "error"; message: string };

export function SessionEndedCard({
  failure,
  resume,
  busy,
  onContinue,
}: {
  failure: PlanUsageFailure;
  resume: ContinueState;
  /** A turn is running, so a second message cannot be sent yet. */
  busy: boolean;
  onContinue: () => void;
}) {
  const signingIn = resume.kind === "signing_in";
  return (
    <div role="alert" data-testid="plan-session-ended-card" data-code="session_ended" className={CARD_CLASS}>
      <p className="flex items-start gap-2">
        <ChatGptLogo tone="on-dark" size={16} className="mt-px" />
        <span>
          <span className="font-medium text-[var(--text)]">{SESSION_ENDED_TITLE}</span> {SESSION_ENDED_BODY}
        </span>
      </p>
      <div className="mt-2 flex flex-wrap items-center gap-2">
        <button
          type="button"
          data-testid="plan-continue"
          disabled={busy || signingIn}
          onClick={onContinue}
          className="btn-primary inline-flex items-center gap-1.5 px-3 py-1 text-xs"
        >
          {/* Approved button: logo and "Continue with ChatGPT". Dark text on
              the violet fill takes the black logo; disabled, the button is a
              dark grey, which takes the white one. */}
          {signingIn ? null : (
            <ChatGptLogo tone={busy ? "on-dark" : "on-light"} size={16} />
          )}
          {signingIn ? SIGNING_IN : CONTINUE_LABEL}
        </button>
      </div>
      {resume.kind === "cancelled" ? (
        <p data-testid="plan-continue-note" className="mt-2">
          {CONTINUE_CANCELLED}
        </p>
      ) : null}
      {resume.kind === "error" ? (
        <p data-testid="plan-continue-error" className="mt-2 break-words text-[var(--danger)]">
          {resume.message}
        </p>
      ) : null}
      <RequestId id={failure.requestId} />
    </div>
  );
}

/**
 * One plain sentence and the request ID, for a failure that has no card of
 * its own. `unavailable` is temporary and offers Try again, which is a click,
 * never an automatic retry; `plan_disabled` offers Settings, where plan usage
 * is turned back on.
 */
export function PlanErrorCard({
  failure,
  onRetry,
  onOpenSettings,
}: {
  failure: PlanUsageFailure;
  onRetry?: () => void;
  onOpenSettings?: () => void;
}) {
  const code = failure.code;
  if (code === "usage_limit" || code === "session_ended") return null;
  return (
    <div
      role="alert"
      data-testid="plan-error-card"
      data-code={code}
      className="banner-in rounded-md border border-[var(--danger-border)] bg-[var(--danger-soft)] px-3 py-2 text-xs text-[var(--danger)]"
    >
      <div className="flex items-start justify-between gap-2">
        <span className="flex items-start gap-2">
          <ChatGptLogo tone="on-dark" size={16} className="mt-px" />
          <span data-testid="plan-error-sentence">{PLAN_ERROR_SENTENCES[code]}</span>
        </span>
        {code === "unavailable" && onRetry ? (
          <button
            type="button"
            data-testid="plan-error-retry"
            onClick={onRetry}
            className="btn-danger shrink-0 rounded px-2 py-0.5 font-mono text-[10px] uppercase tracking-wider"
          >
            Try again
          </button>
        ) : null}
        {code === "plan_disabled" && onOpenSettings ? (
          <button
            type="button"
            data-testid="plan-error-open-settings"
            onClick={onOpenSettings}
            className="btn-danger shrink-0 rounded px-2 py-0.5 font-mono text-[10px] uppercase tracking-wider"
          >
            Open Settings
          </button>
        ) : null}
      </div>
      <RequestId id={failure.requestId} />
    </div>
  );
}
