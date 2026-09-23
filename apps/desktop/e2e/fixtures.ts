/**
 * The `app` fixture: boot the real frontend against a stated backend.
 *
 * Any uncaught exception in the page fails the test that caused it.
 * An exception nothing asserted on is how a blank window ships — the
 * first boot of this harness, given no answers, threw
 * `Cannot read properties of undefined (reading 'length')` and rendered
 * nothing at all, and no assertion about a button would have said why.
 */

import { expect, test as base, type Page } from "@playwright/test";

import type { Backend } from "./backend";

export interface App {
  page: Page;
  /** Load the app with the backend answering as `backend` says. */
  boot(backend: Backend): Promise<void>;
  /** Every command the app has called, in order. */
  calls(): Promise<string[]>;
  /** Commands the app called that the backend had no answer for. */
  unhandled(): Promise<string[]>;
  /** Raise a backend event, as the Rust side would. */
  emit(event: string, payload?: unknown): Promise<void>;
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
 * happened to reach it, and it failed on one run and passed on the
 * next: mounting the timeline calls `save_view_state` after a 500 ms
 * debounce, so whether it had happened yet was a race. Quiescence — no
 * new call for `quietMs` — is a point the app actually reaches, so a
 * check taken there gives the same verdict every run.
 */
async function settle(page: Page, quietMs = 750, timeoutMs = 10_000): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  let last = -1;
  let stableSince = Date.now();
  while (Date.now() < deadline) {
    const n = await page.evaluate(() => window.__E2E_CALLS__?.length ?? 0);
    if (n !== last) {
      last = n;
      stableSince = Date.now();
    } else if (Date.now() - stableSince >= quietMs) {
      return;
    }
    await page.waitForTimeout(100);
  }
  throw new Error(`the app was still calling the backend after ${timeoutMs} ms`);
}

export const test = base.extend<{ app: App }>({
  app: async ({ page }, use) => {
    const pageErrors: string[] = [];
    page.on("pageerror", (err) => pageErrors.push(err.message));
    let booted = false;

    const app: App = {
      page,
      async boot(backend) {
        await page.addInitScript((b) => {
          (window as unknown as { __E2E__: unknown }).__E2E__ = { backend: b };
        }, backend);
        await page.goto("/e2e/harness.html");
        await page.waitForFunction(() => window.__E2E_READY__ === true);
        booted = true;
      },
      calls: () => page.evaluate(() => window.__E2E_CALLS__.map((c) => c.cmd)),
      unhandled: () =>
        page.evaluate(() => [...new Set(window.__E2E_UNHANDLED__)]),
      emit: (event, payload) =>
        page.evaluate(([e, p]) => window.__E2E_EMIT__(e, p), [event, payload] as const),
      become: (answers) =>
        page.evaluate((a) => {
          Object.assign(
            (window as unknown as { __E2E__: { backend: object } }).__E2E__.backend,
            a,
          );
        }, answers),
    };

    await use(app);

    expect(pageErrors, "uncaught exceptions in the page").toEqual([]);
    // Every command the app called had an answer. Checked here, once
    // the app has gone quiet, rather than in each test at an arbitrary
    // moment — see `settle`. A booted page only: a test that never
    // booted has no harness to ask.
    if (booted) {
      await settle(page);
      expect(
        await app.unhandled(),
        "commands the app called that the backend was not told how to answer",
      ).toEqual([]);
    }
  },
});

export { expect };
