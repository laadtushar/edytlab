/**
 * The real app, in a real browser, with only the Rust side replaced.
 *
 * Vitest runs components in jsdom, which has no layout, no Web Audio
 * and no shadow DOM that anything renders into — so a whole class of
 * bug cannot be seen there at all. WaveSurfer draws and scrolls inside
 * its own shadow root, and a component that sets `scrollLeft` on the
 * wrong element passes every jsdom test there is.
 *
 * So this page mounts the unmodified `src/main.tsx` in Chromium, from a
 * production build (see `vite.config.ts`), and replaces exactly one
 * thing: the IPC boundary to the backend, using Tauri's own
 * `@tauri-apps/api/mocks` rather than a hand-rolled stand-in.
 * Everything above that boundary — React, WaveSurfer, decoding, layout,
 * scrolling — is the code that ships.
 *
 * A test supplies the backend's answers before the page loads, as
 * `window.__E2E__.backend`, keyed by command name (see `backend.ts`).
 * Every call is recorded, and so is every command that had no answer:
 * a command the test did not anticipate is a fact about the app worth
 * seeing, not something to paper over with a default.
 */

import { emit } from "@tauri-apps/api/event";
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";

import type { Backend } from "./backend";
import { FILE_ROUTE } from "./routes";

declare global {
  interface Window {
    __E2E__?: { backend: Backend };
    /** The name of every command the app called, in order. */
    __E2E_CALLS__: string[];
    __E2E_UNHANDLED__: string[];
    /** Raise a backend event, as the Rust side would. */
    __E2E_EMIT__: (event: string, payload?: unknown) => Promise<void>;
    __E2E_READY__: boolean;
  }
}

const config = window.__E2E__ ?? { backend: {} };
window.__E2E_CALLS__ = [];
window.__E2E_UNHANDLED__ = [];

mockWindows("main");
mockIPC(
  (cmd) => {
    window.__E2E_CALLS__.push(cmd);
    if (Object.prototype.hasOwnProperty.call(config.backend, cmd)) {
      const answer = config.backend[cmd];
      // A string, not an `Error`. Every command returns
      // `CmdResult<T> = Result<T, String>`, so a failure reaches the
      // frontend as the bare message — and an error path tested
      // against an `Error` object is tested against a shape production
      // never produces.
      if ("reject" in answer) return Promise.reject(answer.reject);
      // A copy, so a component that mutates what it was given cannot
      // change the answer the next caller gets.
      return structuredClone(answer.ok);
    }
    window.__E2E_UNHANDLED__.push(cmd);
    return undefined;
  },
  { shouldMockEvents: true },
);

// Tauri's asset protocol, served by `vite preview` instead. The backend
// hands the app an absolute path exactly as it would in production, and
// it is encoded the way Tauri's own `convertFileSrc` encodes it.
// `mockConvertFileSrc` is not used because it produces an
// `asset://localhost/` URL, which no browser outside Tauri can fetch.
(
  window as unknown as {
    __TAURI_INTERNALS__: { convertFileSrc: (path: string) => string };
  }
).__TAURI_INTERNALS__.convertFileSrc = (path) => FILE_ROUTE + encodeURIComponent(path);

window.__E2E_EMIT__ = (event, payload) => emit(event, payload);

await import("../src/main.tsx");
window.__E2E_READY__ = true;
