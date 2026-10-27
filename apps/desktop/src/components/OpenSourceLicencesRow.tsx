/**
 * "Open source licences" in Settings → Account: the third-party notices file
 * the installer carries, shown in a scrollable panel.
 *
 * The licences of the libraries edytlab is built from ask that their text and
 * copyright lines travel with the program; this is where a person finds them
 * in the app. The text is read from the file in the app's resource directory
 * (`third_party_notices`, read-only, no argument) only when Show is pressed,
 * so opening Settings reads nothing, and it is drawn as plain text in a
 * `<pre>`, never as HTML or markdown, because it is a text file.
 */

import { useState } from "react";

import { thirdPartyNotices } from "../lib/tauri-bridge";

type View =
  | { state: "closed" }
  | { state: "loading" }
  | { state: "shown"; text: string }
  | { state: "failed"; reason: string };

export function OpenSourceLicencesRow() {
  const [view, setView] = useState<View>({ state: "closed" });

  const show = () => {
    setView({ state: "loading" });
    thirdPartyNotices().then(
      (text) => setView({ state: "shown", text }),
      (e: unknown) =>
        setView({
          state: "failed",
          reason: e instanceof Error ? e.message : String(e),
        }),
    );
  };

  const open = view.state === "shown";

  return (
    <div
      data-testid="settings-licences"
      className="mt-3 border-t border-[var(--border)] pt-3"
    >
      <div className="flex items-center justify-between gap-3">
        <div>
          <div className="font-mono text-[10px] uppercase tracking-[0.18em] text-[var(--text-faint)]">
            Open source licences
          </div>
          <p className="mt-1 text-xs text-[var(--text-dim)]">
            edytlab is MIT-licensed and built from open source libraries,
            fonts and code. Their licences and copyright notices are here.
          </p>
        </div>
        <button
          type="button"
          data-testid="settings-licences-toggle"
          aria-expanded={open}
          disabled={view.state === "loading"}
          onClick={() => (open ? setView({ state: "closed" }) : show())}
          className="
            shrink-0 rounded-md border border-[var(--border)] px-2.5 py-1
            text-xs text-[var(--text-dim)] transition hover:text-[var(--text)]
            disabled:opacity-50
          "
        >
          {open ? "Hide" : view.state === "loading" ? "Loading…" : "Show"}
        </button>
      </div>
      {view.state === "failed" ? (
        <p
          data-testid="settings-licences-error"
          role="alert"
          className="mt-2 text-xs text-[var(--warning)]"
        >
          {view.reason}
        </p>
      ) : null}
      {view.state === "shown" ? (
        <pre
          data-testid="settings-licences-text"
          // Keyboard-scrollable: a scroll container with no focus stop cannot
          // be reached without a mouse.
          tabIndex={0}
          aria-label="Third-party notices"
          className="
            mt-2 max-h-64 overflow-auto whitespace-pre-wrap break-words
            rounded-md border border-[var(--border)] bg-[var(--surface)] p-2
            font-mono text-[10px] leading-snug text-[var(--text-dim)]
          "
        >
          {view.text}
        </pre>
      ) : null}
    </div>
  );
}
