/**
 * The `app` fixture: boot the real frontend against a stated backend.
 *
 * Two checks run after every test, once the app has gone quiet:
 *
 * - **No uncaught exception in the page.** An exception nothing asserted
 *   on is how a blank window ships — the first boot of this harness,
 *   given no answers, threw `Cannot read properties of undefined
 *   (reading 'length')` and rendered nothing at all.
 * - **No command the backend was not told how to answer.** A command a
 *   test did not anticipate is a fact about the app, not something a
 *   default should paper over.
 */

import { expect, test as base, type Page } from "@playwright/test";

import type { Backend } from "./backend";

/**
 * How long the app must go without calling the backend before a test
 * counts it as finished.
 *
 * It has to outlast the longest delay the app waits before calling the
 * backend, or a call still pending is missed. Those delays are the
 * selection-context debounce (250 ms), the Settings model catalogue
 * (400 ms) and view-state persistence (500 ms). The first version waited
 * 750 ms, which passed but left 250 ms of margin, so this waits three
 * times the longest of them.
 */
const QUIET_MS = 1_500;
const SETTLE_TIMEOUT_MS = 15_000;

export interface App {
  page: Page;
  /** Load the app with the backend answering as `backend` says. */
  boot(backend: Backend): Promise<void>;
  /** Raise a backend event, as the Rust side would. */
  emit(event: string, payload?: unknown): Promise<void>;
  /**
   * The arguments of every call the app made to `cmd`, in order. What
   * the app asks of the backend is its half of the IPC contract, and the
   * backend is the one boundary these tests fake.
   */
  requestsFor(cmd: string): Promise<unknown[]>;
  /**
   * Answer a call that was given a `deferred` answer, once the app has
   * made it. This is how a test fixes the order of two events instead of
   * racing them.
   */
  release(name: string, value: unknown): Promise<void>;
  /**
   * Wait until the app has stopped calling the backend (see `QUIET_MS`).
   * For asserting that something did *not* happen: a debounced call
   * that was going to be made has been made by then.
   */
  settle(): Promise<void>;
  /**
   * Change what the backend answers from now on — the state after an
   * edit, say. The mock reads its answers live, so the next call sees
   * these.
   */
  become(answers: Backend): Promise<void>;
}

/**
 * Wait until the app has stopped calling the backend.
 *
 * The unanswered-command check used to run at whatever moment a test
 * reached it, and it failed on one run and passed on the next: the
 * timeline calls `save_view_state` after a 500 ms debounce, and whether
 * that had happened yet was a race. Checking once the app is quiet (see
 * `QUIET_MS`) gives the same verdict every run.
 */
async function settle(page: Page): Promise<void> {
  const deadline = Date.now() + SETTLE_TIMEOUT_MS;
  let last = -1;
  let quietSince = Date.now();
  while (Date.now() < deadline) {
    const n = await page.evaluate(() => window.__E2E_CALLS__.length);
    if (n !== last) {
      last = n;
      quietSince = Date.now();
    } else if (Date.now() - quietSince >= QUIET_MS) {
      return;
    }
    await page.waitForTimeout(100);
  }
  throw new Error(`the app was still calling the backend after ${SETTLE_TIMEOUT_MS} ms`);
}

export const test = base.extend<{ app: App }>({
  app: async ({ page }, use) => {
    const pageErrors: string[] = [];
    page.on("pageerror", (err) => pageErrors.push(err.message));
    let booted = false;
    // A deferred answer the test never releases leaves a call pending
    // for good, and the test passes anyway. Checked at teardown.
    const released = new Set<string>();

    const app: App = {
      page,
      async boot(backend) {
        await page.addInitScript((b) => {
          window.__E2E__ = { backend: b };
        }, backend);
        await page.goto("/e2e/harness.html");
        await page.waitForFunction(() => window.__E2E_READY__ === true);
        booted = true;
      },
      requestsFor: (cmd) =>
        page.evaluate(
          (c) => window.__E2E_CALLS__.filter((call) => call.cmd === c).map((call) => call.args),
          cmd,
        ),
      settle: () => settle(page),
      async release(name, value) {
        await page.waitForFunction((n) => n in window.__E2E_DEFERRED__, name);
        await page.evaluate(([n, v]) => window.__E2E_DEFERRED__[n](v), [name, value] as const);
        released.add(name);
      },
      emit: (event, payload) =>
        page.evaluate(([e, p]) => window.__E2E_EMIT__(e, p), [event, payload] as const),
      become: (answers) =>
        page.evaluate((a) => {
          Object.assign(window.__E2E__!.backend, a);
        }, answers),
    };

    await use(app);

    // Settle first, and only then check. Checking page errors before
    // waiting let an exception thrown during the wait go unrecorded,
    // which a review proved with a `throw` scheduled 300 ms after the
    // test body ended.
    if (booted) await settle(page);
    expect(pageErrors, "uncaught exceptions in the page").toEqual([]);
    if (booted) {
      const pending = (await page.evaluate(() => Object.keys(window.__E2E_DEFERRED__))).filter(
        (name) => !released.has(name),
      );
      expect(pending, "deferred answers the test never released").toEqual([]);
      const unhandled = await page.evaluate(() => [...new Set(window.__E2E_UNHANDLED__)]);
      expect(unhandled, "commands the app called that the backend was not told how to answer").toEqual([]);
    }
  },
});

export { expect };
