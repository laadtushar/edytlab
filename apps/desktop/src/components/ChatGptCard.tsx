/**
 * The ChatGPT plan in Settings → Account: a card, not a key field (#474 PR 6).
 *
 * The plan has no key. A person signs in with ChatGPT in their browser, and
 * the backend keeps the session (`chatgpt.rs`); this card shows where that
 * stands and offers what can be done next. Nothing here holds a token or sees
 * a URL: the browser is opened by the backend, and no command used here
 * returns either.
 *
 * States, from the backend's `ChatGptStatus.state`:
 *
 * - signed out: what it does, who it is for, and **Continue with ChatGPT**;
 *   after a sign-out or a start over, what the backend says (`notice`) and
 *   that signing out here does not disconnect edytlab from ChatGPT (G16);
 * - signing in: "Finish signing in in your browser…" and Cancel; after about
 *   20 s, what a browser error page may have said (G6), because a page that
 *   OpenAI shows never reaches edytlab and the wait lasts five minutes;
 * - signed in: the account, "Using ChatGPT plan", the account's models, the
 *   reasoning levels of the chosen model, Manage usage, Sign out, Use another
 *   account and Start over;
 * - plan usage declined: **Turn plan usage back on**;
 * - session ended: the reason, and **Continue with ChatGPT**;
 * - unread and unavailable: a sentence saying why nothing can be done yet.
 *
 * The status arrives on `chatgpt://status`. The card subscribes before it
 * asks, so a change between the two is not lost, and an answer to one of its
 * own commands is dropped when an event arrived while it was on the way.
 */

import { useCallback, useEffect, useRef, useState } from "react";

import {
  CHATGPT_LABEL,
  CONTINUE_LABEL,
  DISCONNECT_LINE,
  ELIGIBILITY_NOTE,
  ENABLE_PLAN_USAGE_LABEL,
  MANAGE_USAGE_LABEL,
  MODEL_NOT_LISTED,
  PLAN_USAGE_DECLINED,
  SESSION_ENDED,
  SIGNED_OUT_ABOUT,
  SIGNING_HINT,
  SIGNING_HINT_AFTER_MS,
  SIGNING_IN,
  START_OVER_QUESTION,
  UNAVAILABLE,
  UNREAD,
  USING_PLAN,
  effortNotOffered,
  signedInAs,
} from "../lib/chatgptCopy";
import { confirmAction } from "../lib/confirmAction";
import { EFFORT_OPTIONS, asEffort, levelsOffered } from "../lib/effort";
import { chatGptStorageNotice, savedNotice } from "../lib/keychainNotice";
import { modelStorageKey } from "../lib/model-storage";
import {
  chatgptCancelSignIn,
  chatgptForgetRegistration,
  chatgptSignIn,
  chatgptSignOut,
  chatgptStatus,
  getActiveModel,
  getEffortFor,
  listModelsFor,
  onChatGptStatus,
  openChatgptUsage,
  setActiveModel,
  setEffortFor,
  type ChatGptEnd,
  type ChatGptSignInIntent,
  type ChatGptStatus,
  type KeychainPersistence,
  type ModelInfo,
  type ReasoningEffort,
} from "../lib/tauri-bridge";

/** A small turning ring beside a button's label while its action runs. */
function Spinner() {
  return (
    <span
      aria-hidden="true"
      data-testid="chatgpt-spinner"
      className="mr-1.5 inline-block h-3 w-3 animate-spin rounded-full border-2 border-current border-t-transparent align-[-2px]"
    />
  );
}

function messageOf(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

/** Every state the card draws, with the two it has before and without a status. */
const KNOWN_STATES: readonly string[] = [
  "loading",
  "error",
  "signed_out",
  "signing_in",
  "signed_in",
  "plan_disabled",
  "session_ended",
  "unread",
  "unavailable",
];

const LABEL_CLASS =
  "mb-1.5 block font-mono text-[10px] uppercase tracking-[0.18em] text-[var(--text-faint)]";

export interface ChatGptCardProps {
  /**
   * The person finished a sign-in they started here. Settings makes the plan
   * the active provider. A rejection is shown on the card.
   */
  onSignedIn?: () => void | Promise<void>;
  /** The person signed out or started over: the plan has no session now. */
  onSignedOut?: () => void;
}

export function ChatGptCard({ onSignedIn, onSignedOut }: ChatGptCardProps) {
  const [status, setStatus] = useState<ChatGptStatus | null>(null);
  // The status could not be read at all.
  const [readError, setReadError] = useState<string | null>(null);
  // A sign-in this card started and has not heard the end of. The status says
  // `signing_in` too, once the browser is about to open; this covers the time
  // before it does.
  const [signingLocal, setSigningLocal] = useState(false);
  const [cancelling, setCancelling] = useState(false);
  const [hint, setHint] = useState(false);
  // A command that was refused, or a sign-in that failed: a sentence.
  const [problem, setProblem] = useState<string | null>(null);
  // What the last sign-out or start over said, for the signed-out card.
  const [ended, setEnded] = useState<ChatGptEnd | null>(null);
  const [busy, setBusy] = useState(false);
  // Which of the ending actions is running, for the spinner on its own button.
  const [ending, setEnding] = useState<"sign_out" | "start_over" | null>(null);

  const alive = useRef(true);
  // Counts the events heard, so an answer that was overtaken by one is not
  // allowed to put an older state back.
  const heard = useRef(0);

  const read = useCallback(() => {
    setReadError(null);
    chatgptStatus().then(
      (next) => {
        if (alive.current) setStatus(next);
      },
      (e: unknown) => {
        if (alive.current) setReadError(messageOf(e));
      },
    );
  }, []);

  useEffect(() => {
    alive.current = true;
    // Per run of the effect, not the shared ref: under StrictMode the first
    // run's listener registers after the second run has set `alive` again,
    // and has to be let go all the same.
    let live = true;
    let unlisten: (() => void) | null = null;

    const subscribed = onChatGptStatus((next) => {
      if (!live) return;
      heard.current += 1;
      setStatus(next);
    }).then(
      (fn) => {
        if (live) unlisten = fn;
        else fn();
      },
      // No event channel: the card still shows, it just does not move on its own.
      () => undefined,
    );

    void subscribed
      .then(() => chatgptStatus())
      .then(
        (first) => {
          // An event that came first is newer than this answer.
          if (live && heard.current === 0) setStatus(first);
        },
        (e: unknown) => {
          if (live) setReadError(messageOf(e));
        },
      );

    return () => {
      live = false;
      alive.current = false;
      unlisten?.();
    };
  }, []);

  const signing = signingLocal || status?.state === "signing_in";

  // The hint under Cancel (G6), after the sign-in has waited a while.
  useEffect(() => {
    if (!signing) {
      setHint(false);
      return;
    }
    const timer = window.setTimeout(() => setHint(true), SIGNING_HINT_AFTER_MS);
    return () => window.clearTimeout(timer);
  }, [signing]);

  const signIn = useCallback(
    async (intent: ChatGptSignInIntent) => {
      setProblem(null);
      setEnded(null);
      setCancelling(false);
      setSigningLocal(true);
      try {
        const next = await chatgptSignIn(intent);
        if (!alive.current) return;
        setStatus(next);
        if (next.state === "signed_in") {
          try {
            await onSignedIn?.();
          } catch (e) {
            if (alive.current) {
              setProblem(`Signed in, but ChatGPT could not be made the active provider: ${messageOf(e)}`);
            }
          }
        }
      } catch (e) {
        if (!alive.current) return;
        setProblem(messageOf(e));
        // The failure leaves the status as it was, but the last event heard
        // may have been `signing_in`.
        read();
      } finally {
        if (alive.current) {
          setSigningLocal(false);
          setCancelling(false);
        }
      }
    },
    [onSignedIn, read],
  );

  const cancel = () => {
    setCancelling(true);
    chatgptCancelSignIn().catch((e: unknown) => {
      if (alive.current) {
        setCancelling(false);
        setProblem(messageOf(e));
      }
    });
  };

  const end = useCallback(
    async (call: () => Promise<ChatGptEnd>, which: "sign_out" | "start_over") => {
      setProblem(null);
      setBusy(true);
      setEnding(which);
      try {
        const done = await call();
        if (!alive.current) return;
        setStatus(done.status);
        setEnded(done);
        onSignedOut?.();
      } catch (e) {
        if (alive.current) setProblem(messageOf(e));
      } finally {
        if (alive.current) {
          setBusy(false);
          setEnding(null);
        }
      }
    },
    [onSignedOut],
  );

  const startOver = async () => {
    let confirmed = false;
    try {
      confirmed = await confirmAction(START_OVER_QUESTION);
    } catch (e) {
      // The dialog could not be shown: that is not a yes.
      if (alive.current) setProblem(messageOf(e));
      return;
    }
    if (confirmed) await end(chatgptForgetRegistration, "start_over");
  };

  const manageUsage = () => {
    setProblem(null);
    openChatgptUsage().catch((e: unknown) => {
      if (alive.current) setProblem(messageOf(e));
    });
  };

  const state: string = signing
    ? "signing_in"
    : (status?.state ?? (readError === null ? "loading" : "error"));
  // A state this build does not know (a newer backend's, such as one for a
  // start over in progress) gets a neutral line, not an empty card.
  const known = KNOWN_STATES.includes(state);
  const storage = chatGptStorageNotice(status?.tokenStorage ?? null);

  const continueButton = (label = CONTINUE_LABEL, intent: ChatGptSignInIntent = "sign_in") => (
    <button
      type="button"
      data-testid="chatgpt-continue"
      onClick={() => void signIn(intent)}
      className="btn-primary px-4 py-1.5 text-sm transition"
    >
      {label}
    </button>
  );

  return (
    <div
      data-testid="settings-chatgpt-card"
      data-state={state}
      aria-label={CHATGPT_LABEL}
      className="mb-4 rounded-md border border-[var(--border)] bg-[var(--surface)] px-3 py-3"
    >
      {!known ? (
        <p data-testid="chatgpt-status" className="font-mono text-[11px] text-[var(--text-faint)]">
          Working…
        </p>
      ) : null}

      {state === "loading" ? (
        <p data-testid="chatgpt-status" className="font-mono text-[11px] text-[var(--text-faint)]">
          Checking your ChatGPT sign-in…
        </p>
      ) : null}

      {state === "error" ? (
        <div className="flex flex-wrap items-center gap-2">
          <p data-testid="chatgpt-status" className="break-words text-xs text-[var(--warning)]">
            Could not check your ChatGPT sign-in ({readError})
          </p>
          <button
            type="button"
            data-testid="chatgpt-check"
            onClick={read}
            className="btn-secondary rounded px-2 py-0.5 text-[10px] uppercase tracking-wider transition"
          >
            Check again
          </button>
        </div>
      ) : null}

      {state === "signing_in" ? (
        <div>
          <p data-testid="chatgpt-status" className="mb-2 text-sm text-[var(--text)]">
            {SIGNING_IN}
          </p>
          <button
            type="button"
            data-testid="chatgpt-cancel"
            disabled={cancelling}
            onClick={cancel}
            className="btn-secondary rounded px-3 py-1 text-xs transition"
          >
            {cancelling ? "Cancelling…" : "Cancel"}
          </button>
          {hint ? (
            <p
              data-testid="chatgpt-signing-hint"
              role="note"
              className="mt-3 text-xs leading-relaxed text-[var(--text-dim)]"
            >
              {SIGNING_HINT}
            </p>
          ) : null}
        </div>
      ) : null}

      {state === "signed_out" ? (
        <div>
          {ended ? (
            <div data-testid="chatgpt-end-notice" role="status" className="mb-3 text-xs leading-relaxed text-[var(--text-dim)]">
              {ended.notice ? <p className="mb-1.5">{ended.notice}</p> : null}
              <p>{DISCONNECT_LINE}</p>
            </div>
          ) : null}
          <p className="mb-1 text-sm text-[var(--text)]">{SIGNED_OUT_ABOUT}</p>
          <p data-testid="chatgpt-eligibility" className="mb-3 text-xs text-[var(--text-faint)]">
            {ELIGIBILITY_NOTE}
          </p>
          {continueButton()}
        </div>
      ) : null}

      {state === "session_ended" ? (
        <div>
          <p data-testid="chatgpt-status" className="mb-1 text-sm text-[var(--text)]">
            {SESSION_ENDED}
          </p>
          {status?.reason ? (
            <p data-testid="chatgpt-reason" className="mb-3 text-xs text-[var(--text-dim)]">
              {status.reason.message}
            </p>
          ) : (
            <div className="mb-3" />
          )}
          {continueButton()}
        </div>
      ) : null}

      {state === "plan_disabled" ? (
        <div>
          {status?.email ? (
            <p data-testid="chatgpt-account" className="mb-1 text-xs text-[var(--text-dim)]">
              {signedInAs(status.email)}
            </p>
          ) : null}
          <p data-testid="chatgpt-status" className="mb-3 text-sm text-[var(--text)]">
            {PLAN_USAGE_DECLINED}
          </p>
          <div className="flex flex-wrap gap-2">
            {continueButton(ENABLE_PLAN_USAGE_LABEL, "enable_plan_usage")}
            <button
              type="button"
              data-testid="chatgpt-sign-out"
              disabled={busy}
              onClick={() => void end(chatgptSignOut, "sign_out")}
              className="btn-secondary rounded px-3 py-1 text-xs transition"
            >
              Sign out
            </button>
          </div>
        </div>
      ) : null}

      {state === "unread" ? (
        <div>
          <p data-testid="chatgpt-status" className="mb-1 text-sm text-[var(--text)]">
            {UNREAD}
          </p>
          {status?.reason ? (
            <p data-testid="chatgpt-reason" className="mb-3 text-xs text-[var(--text-dim)]">
              {status.reason.message}
            </p>
          ) : (
            <div className="mb-3" />
          )}
          {continueButton()}
        </div>
      ) : null}

      {state === "unavailable" ? (
        <div>
          <p data-testid="chatgpt-status" className="text-sm text-[var(--text)]">
            {UNAVAILABLE}
          </p>
          {status?.reason ? (
            <p data-testid="chatgpt-reason" className="mt-1 text-xs text-[var(--text-dim)]">
              {status.reason.message}
            </p>
          ) : null}
        </div>
      ) : null}

      {state === "signed_in" && status ? (
        <div>
          <div className="mb-3 flex flex-wrap items-center justify-between gap-2">
            <p data-testid="chatgpt-account" className="min-w-0 break-words text-sm text-[var(--text)]">
              {signedInAs(status.email)}
            </p>
            <span
              data-testid="chatgpt-using-plan"
              className="rounded-full border border-[var(--accent-border)] bg-[var(--accent-soft)] px-2 py-0.5 font-mono text-[10px] uppercase tracking-wider text-[var(--accent-text)]"
            >
              {USING_PLAN}
            </span>
          </div>

          <PlanPickers key={status.email ?? ""} onProblem={setProblem} />

          <div className="mt-1 flex flex-wrap items-center gap-2">
            <button
              type="button"
              data-testid="chatgpt-manage-usage"
              onClick={manageUsage}
              className="btn-secondary rounded px-3 py-1 text-xs transition"
            >
              {MANAGE_USAGE_LABEL}
            </button>
            <button
              type="button"
              data-testid="chatgpt-sign-out"
              disabled={busy}
              onClick={() => void end(chatgptSignOut, "sign_out")}
              className="btn-secondary rounded px-3 py-1 text-xs transition"
            >
              {ending === "sign_out" ? <Spinner /> : null}
              {ending === "sign_out" ? "Signing out…" : "Sign out"}
            </button>
            <button
              type="button"
              data-testid="chatgpt-another-account"
              disabled={busy}
              onClick={() => void signIn("another_account")}
              className="btn-secondary rounded px-3 py-1 text-xs transition"
            >
              Use another account
            </button>
            <button
              type="button"
              data-testid="chatgpt-start-over"
              disabled={busy}
              onClick={() => void startOver()}
              className="btn-secondary rounded px-3 py-1 text-xs transition"
            >
              {ending === "start_over" ? <Spinner /> : null}
              {ending === "start_over" ? "Starting over…" : "Start over"}
            </button>
          </div>
        </div>
      ) : null}

      {storage ? (
        <p
          data-testid="chatgpt-storage-notice"
          role="note"
          className="mt-3 rounded-md border border-[var(--warning)]/40 bg-[var(--warning)]/10 px-3 py-1.5 text-xs text-[var(--warning)]"
        >
          {storage.message}
        </p>
      ) : null}

      {problem ? (
        <p
          data-testid="chatgpt-error"
          role="alert"
          className="mt-3 break-words rounded-md border border-[var(--danger)]/40 bg-[var(--danger)]/10 px-3 py-1.5 text-xs text-[var(--danger)]"
        >
          {problem}
        </p>
      ) : null}
    </div>
  );
}

interface PickersLoaded {
  models: ModelInfo[];
  /** The model chosen for the plan, "" when none, null when it could not be read. */
  chosen: string | null;
  effort: ReasoningEffort | null;
  /** Why the account's list could not be read. */
  modelsError: string | null;
}

/**
 * The model and the reasoning level of a signed-in account.
 *
 * The models are the account's own (`list_models_for("chatgpt")`): which are
 * offered differs by plan, so none is assumed. A person who has chosen
 * nothing is given the first the account lists, because the backend's
 * fallback may not be one of them. The levels are those the chosen model
 * lists (`ModelInfo.reasoning_levels`); a model that lists none gets no
 * picker, and the backend sends it no effort either. Both save at once, since
 * there is no key to Save beside them.
 */
function PlanPickers({ onProblem }: { onProblem: (message: string | null) => void }) {
  const [loaded, setLoaded] = useState<PickersLoaded | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const alive = useRef(true);

  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);

  useEffect(() => {
    let live = true;
    void (async () => {
      const [catalogue, chosen, effort] = await Promise.all([
        listModelsFor("chatgpt").then(
          (c) => ({ models: c.models, error: null as string | null }),
          (e: unknown) => ({ models: [] as ModelInfo[], error: messageOf(e) }),
        ),
        getActiveModel("chatgpt").then(
          (m) => m.trim(),
          () => null,
        ),
        getEffortFor("chatgpt").then(
          (e) => asEffort(e) || null,
          () => null,
        ),
      ]);
      if (!live) return;
      let pick = chosen;
      // Nothing chosen yet: the account's first listed model, saved, so what
      // the backend sends is one the account has.
      if (chosen === "" && catalogue.models.length > 0) {
        pick = catalogue.models[0].id;
        try {
          await setActiveModel("chatgpt", pick);
          rememberModel(pick);
        } catch (e) {
          if (live) onProblem(`could not select ${pick}: ${messageOf(e)}`);
          pick = "";
        }
        if (!live) return;
      }
      setLoaded({ models: catalogue.models, chosen: pick, effort, modelsError: catalogue.error });
    })();
    return () => {
      live = false;
    };
    // Once per sign-in: the card remounts this when the account changes.
  }, []);

  if (!loaded) {
    return (
      <p data-testid="chatgpt-models-loading" className="mb-3 font-mono text-[11px] text-[var(--text-faint)]">
        Loading your account's models…
      </p>
    );
  }

  const { models, chosen, effort, modelsError } = loaded;
  const listed = chosen !== null && chosen !== "" && models.some((m) => m.id === chosen);
  const current = models.find((m) => m.id === chosen);
  const levels = current ? levelsOffered(current.reasoning_levels) : [];
  const effortListed = effort !== null && levels.includes(effort);

  const savedAt = (at: KeychainPersistence | null | undefined) => {
    const notice = savedNotice([at]);
    setNote(notice ? notice.message : null);
  };

  const chooseModel = async (id: string) => {
    if (!id || id === chosen) return;
    const before = loaded;
    setLoaded({ ...loaded, chosen: id });
    onProblem(null);
    try {
      const at = await setActiveModel("chatgpt", id);
      rememberModel(id);
      if (alive.current) savedAt(at);
    } catch (e) {
      if (alive.current) {
        setLoaded(before);
        onProblem(`could not select ${id}: ${messageOf(e)}`);
      }
    }
  };

  const chooseEffort = async (value: string) => {
    const next = asEffort(value) || null;
    const before = loaded;
    setLoaded({ ...loaded, effort: next });
    onProblem(null);
    try {
      const at = await setEffortFor("chatgpt", next);
      if (alive.current) savedAt(at);
    } catch (e) {
      if (alive.current) {
        setLoaded(before);
        onProblem(`could not save the reasoning level: ${messageOf(e)}`);
      }
    }
  };

  return (
    <div className="mb-3">
      <label htmlFor="chatgpt-model-select" className={LABEL_CLASS}>
        Model
      </label>
      {models.length > 0 ? (
        <select
          id="chatgpt-model-select"
          data-testid="chatgpt-model-select"
          aria-label="Model"
          value={chosen ?? ""}
          onChange={(e) => void chooseModel(e.target.value)}
          className="field mb-1 w-full px-3 py-2 text-sm outline-none transition"
        >
          {chosen === null || chosen === "" ? (
            <option value="" disabled>
              Choose a model
            </option>
          ) : null}
          {chosen && !listed ? <option value={chosen}>{chosen}</option> : null}
          {models.map((m) => (
            <option key={m.id} value={m.id}>
              {m.display_name && m.display_name !== m.id ? `${m.display_name} (${m.id})` : m.id}
            </option>
          ))}
        </select>
      ) : (
        <p data-testid="chatgpt-models-error" className="mb-1 text-xs text-[var(--warning)]">
          {modelsError
            ? `Couldn't read your account's models (${modelsError}).`
            : "Your account lists no models."}
          {chosen ? ` Using ${chosen}.` : ""}
        </p>
      )}
      {models.length > 0 && chosen && !listed ? (
        <p data-testid="chatgpt-model-not-listed" role="status" className="mb-1 text-xs text-[var(--warning)]">
          {MODEL_NOT_LISTED}
        </p>
      ) : null}

      {levels.length > 0 ? (
        <>
          <label htmlFor="chatgpt-effort-select" className={`${LABEL_CLASS} mt-3`}>
            Reasoning effort
          </label>
          <select
            id="chatgpt-effort-select"
            data-testid="chatgpt-effort-select"
            aria-label="Reasoning effort"
            value={effortListed ? effort : ""}
            onChange={(e) => void chooseEffort(e.target.value)}
            className="field mb-1 w-full px-3 py-2 text-sm outline-none transition"
          >
            {EFFORT_OPTIONS.filter((o) => o.value === "" || levels.includes(o.value)).map((o) => (
              <option key={o.value || "default"} value={o.value}>
                {o.label}
              </option>
            ))}
          </select>
          {effort && !effortListed ? (
            <p data-testid="chatgpt-effort-not-offered" role="status" className="mb-1 text-xs text-[var(--warning)]">
              {effortNotOffered(effort)}
            </p>
          ) : null}
        </>
      ) : null}

      {note ? (
        <p
          data-testid="chatgpt-save-note"
          role="note"
          className="mt-2 rounded-md border border-[var(--warning)]/40 bg-[var(--warning)]/10 px-3 py-1.5 text-xs text-[var(--warning)]"
        >
          {note}
        </p>
      ) : null}
    </div>
  );
}

/** What Settings' model field shows, so the two agree. Best effort. */
function rememberModel(id: string) {
  try {
    window.localStorage.setItem(modelStorageKey("chatgpt"), id);
  } catch {
    // Private mode or blocked storage: the backend has it, which is what the
    // next message uses.
  }
}
