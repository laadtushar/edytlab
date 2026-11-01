/**
 * The one-time "You're using your ChatGPT plan" notice (#474, spec 4.5).
 * A plain dialog, no animation, so there is nothing for
 * `prefers-reduced-motion` to switch off. Whether it has been seen is
 * `localStorage` (see `FIRST_USE_STORAGE_KEY`); it never touches a token.
 */

import { useEffect, useRef } from "react";

import { FIRST_USE_BODY, FIRST_USE_DISMISS, FIRST_USE_TITLE } from "../lib/chatgptCopy";

export function ChatGptFirstUseNotice({ onDismiss }: { onDismiss: () => void }) {
  const button = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    button.current?.focus();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        onDismiss();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onDismiss]);
  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60"
      data-testid="chatgpt-first-use"
      role="dialog"
      aria-modal="true"
      aria-labelledby="chatgpt-first-use-title"
      aria-describedby="chatgpt-first-use-body"
    >
      <div className="w-96 rounded-xl border border-[var(--border-strong)] bg-[var(--surface-elev)] p-6 shadow-(--shadow-dialog)">
        <h2 id="chatgpt-first-use-title" className="mb-2 text-sm font-semibold text-[var(--text)]">
          {FIRST_USE_TITLE}
        </h2>
        <p id="chatgpt-first-use-body" className="mb-4 text-sm text-[var(--text-dim)]">
          {FIRST_USE_BODY}
        </p>
        <button
          ref={button}
          type="button"
          onClick={onDismiss}
          className="btn-tool rounded-md px-3 py-1.5 text-xs"
        >
          {FIRST_USE_DISMISS}
        </button>
      </div>
    </div>
  );
}
