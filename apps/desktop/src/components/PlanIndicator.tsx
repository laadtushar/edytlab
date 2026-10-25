/**
 * "Using ChatGPT plan" beside the composer (#474), with the ChatGPT logo and
 * a **Manage usage** link, as OpenAI's UI guidelines ask for an app whose
 * requests spend a person's ChatGPT plan.
 *
 * It shows only while the plan is the active provider *and* the sign-in is
 * live (`signed_in`); otherwise it draws nothing. The active provider is read
 * on mount and again whenever `refreshKey` changes (App bumps it when Settings
 * closes or the provider changes) and on every `chatgpt://status` event, which
 * also covers a sign-out. The sign-in's status is asked for only when the plan
 * is the active provider, so any other provider makes no ChatGPT call. A read
 * that fails hides the line: it is a courtesy, never a reason to stop the
 * composer.
 */

import { useEffect, useState } from "react";

import { MANAGE_USAGE_LABEL, USING_PLAN } from "../lib/chatgptCopy";
import {
  chatgptStatus,
  getActiveProvider,
  onChatGptStatus,
  openChatgptUsage,
  type ChatGptStatus,
} from "../lib/tauri-bridge";
import { ChatGptLogo } from "./ChatGptLogo";

export function PlanIndicator({ refreshKey = 0 }: { refreshKey?: number }) {
  const [active, setActive] = useState(false);
  const [signedIn, setSignedIn] = useState(false);

  useEffect(() => {
    let live = true;
    let unlisten: (() => void) | null = null;
    // Counts reads, so an older answer cannot put an older state back.
    let seq = 0;

    const show = (isActive: boolean, isSignedIn: boolean) => {
      setActive(isActive);
      setSignedIn(isSignedIn);
    };

    // The provider first, and the ChatGPT sign-in only when the plan is the
    // provider: a person on any other provider costs the backend nothing here.
    // `known` is a status that has just arrived on the event.
    const refresh = (known?: ChatGptStatus) => {
      const mine = ++seq;
      const current = () => live && mine === seq;
      try {
        Promise.resolve(getActiveProvider())
          .then((provider) => {
            if (provider !== "chatgpt") return { active: false, signedIn: false };
            if (known) return { active: true, signedIn: known.state === "signed_in" };
            return Promise.resolve(chatgptStatus()).then((s) => ({
              active: true,
              signedIn: s?.state === "signed_in",
            }));
          })
          .then(
            (next) => {
              if (current()) show(next.active, next.signedIn);
            },
            () => {
              if (current()) show(false, false);
            },
          );
      } catch {
        show(false, false);
      }
    };

    try {
      const subscribed = onChatGptStatus((next: ChatGptStatus) => {
        if (live) refresh(next);
      });
      Promise.resolve(subscribed).then(
        (fn) => {
          if (typeof fn !== "function") return;
          if (live) unlisten = fn;
          else fn();
        },
        () => undefined,
      );
    } catch {
      // No event channel: the line is read on mount and when `refreshKey` moves.
    }
    refresh();

    return () => {
      live = false;
      unlisten?.();
    };
  }, [refreshKey]);

  if (!active || !signedIn) return null;

  return (
    <div
      data-testid="plan-indicator"
      className="mb-2 flex items-center justify-between gap-2 px-0.5 text-xs text-[var(--text-dim)]"
    >
      <span className="flex min-w-0 items-center gap-1.5">
        <ChatGptLogo tone="on-dark" size={16} />
        <span data-testid="plan-indicator-text" className="truncate">
          {USING_PLAN}
        </span>
      </span>
      <button
        type="button"
        data-testid="plan-indicator-manage-usage"
        onClick={() => {
          try {
            openChatgptUsage().catch(() => undefined);
          } catch {
            // Nothing to show here; Settings' card reports a failure.
          }
        }}
        className="shrink-0 rounded px-1 text-[var(--text-faint)] underline-offset-2 transition hover:text-[var(--accent-text)] hover:underline"
      >
        {MANAGE_USAGE_LABEL}
      </button>
    </div>
  );
}
