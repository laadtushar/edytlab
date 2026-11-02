/**
 * The webview's half of the opt-in crash reports (#958).
 *
 * An uncaught error (`window.onerror`) or an unhandled promise rejection is
 * handed to the backend with `report_webview_error`, which has the app's one
 * reporting client and the one consent check: with Send crash reports off,
 * or in a build that cannot send, the backend drops it. Nothing here talks to
 * the network, so the CSP is unchanged, and nothing here reads the setting.
 *
 * What is handed over is the error's name, message and stack, nothing else:
 * no page state, no chat, no tool data. The backend scrubs paths, quoted
 * text, keys and email addresses before anything leaves. At most
 * `MAX_REPORTS` errors a launch are forwarded, each distinct one once, so a
 * render loop that throws cannot flood the IPC channel.
 */

import { reportWebviewError, type WebviewErrorReport } from "./tauri-bridge";

/** The most errors forwarded in one launch of the window. */
export const MAX_REPORTS = 20;

/** The longest message or stack handed over; the backend cuts again. */
const MAX_TEXT = 4_000;

type Send = (report: WebviewErrorReport) => Promise<void>;

/** What an `error` event or a rejection's reason says about itself. */
export function reportFrom(
  kind: WebviewErrorReport["kind"],
  reason: unknown,
  fallbackMessage = "",
): WebviewErrorReport {
  if (reason instanceof Error) {
    return {
      kind,
      name: reason.name || "Error",
      message: cut(reason.message || fallbackMessage),
      stack: typeof reason.stack === "string" ? cut(reason.stack) : null,
    };
  }
  // A thrown string, object or nothing: its text is the message, and the
  // backend gives it the name `Error` and removes what it quotes.
  const message =
    typeof reason === "string" ? reason : reason === undefined || reason === null ? fallbackMessage : safeString(reason);
  return { kind, name: "Error", message: cut(message), stack: null };
}

function safeString(value: unknown): string {
  try {
    return String(value);
  } catch {
    return "";
  }
}

function cut(text: string): string {
  return text.length > MAX_TEXT ? text.slice(0, MAX_TEXT) : text;
}

/**
 * Forward the window's uncaught errors to the backend. Returns the function
 * that stops it (for tests; the app keeps it for its lifetime).
 */
export function forwardWebviewErrors(target: Window = window, send: Send = reportWebviewError): () => void {
  const seen = new Set<string>();
  let forwarded = 0;

  const forward = (report: WebviewErrorReport) => {
    const key = `${report.kind}\u0000${report.name}\u0000${report.message}`;
    if (seen.has(key) || forwarded >= MAX_REPORTS) return;
    seen.add(key);
    forwarded += 1;
    // A report that cannot be handed over is not a new error to report:
    // a rejection, or `invoke` throwing at once outside Tauri (a test, a
    // plain browser), ends here.
    // The executor runs at once, so the report is handed over now.
    new Promise<void>((resolve) => {
      resolve(send(report));
    }).catch(() => undefined);
  };

  const onError = (event: ErrorEvent) => {
    forward(reportFrom("error", event.error, event.message));
  };
  const onRejection = (event: PromiseRejectionEvent) => {
    forward(reportFrom("unhandledrejection", event.reason));
  };

  target.addEventListener("error", onError);
  target.addEventListener("unhandledrejection", onRejection);
  return () => {
    target.removeEventListener("error", onError);
    target.removeEventListener("unhandledrejection", onRejection);
  };
}
