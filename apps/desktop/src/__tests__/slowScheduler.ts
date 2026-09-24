/**
 * Run React's scheduler 40 ms late, when vitest is started with
 * `--mode slow-scheduler` (`pnpm test:slow-scheduler`, #349).
 *
 * A test that asserts on what React draws from an async result, without
 * waiting for it, passes whenever React happens to be fast and fails when
 * it is not — which is how a Windows runner turned such a test red on a
 * PR that never touched it (#346). React's scheduler queues its work with
 * `setImmediate` in Node; RTL's `waitFor` returns after a `setTimeout(0)`;
 * Node does not order the two. Delaying only the scheduler makes the
 * slow case the every-time case, so the mistake fails on the PR that
 * makes it.
 *
 * Imported first in setup.ts, ahead of anything that loads React: the
 * scheduler reads `setImmediate` once, when its module is evaluated, and
 * import order is the only thing that runs before that.
 */

import { afterAll } from "vitest";

if (import.meta.env.MODE === "slow-scheduler") {
  // Captured now: a test that installs fake timers must not capture these.
  const later = globalThis.setTimeout;
  const cancel = globalThis.clearTimeout;
  const pending = new Set<ReturnType<typeof setTimeout>>();

  (globalThis as unknown as { setImmediate: unknown }).setImmediate = (
    fn: (...args: unknown[]) => void,
    ...args: unknown[]
  ) => {
    const handle = later(() => {
      pending.delete(handle);
      fn(...args);
    }, 40);
    pending.add(handle);
    return handle;
  };

  // Work still queued when a file's tests are done is dropped, not run.
  // Run late, it landed after vitest had torn the file's jsdom down and
  // failed the whole job on `window is not defined` from inside react-dom
  // — with every test passing (#354's first CI runs). RTL's cleanup has
  // unmounted every tree by then, so the work belongs to nothing; and
  // `afterAll` runs before the environment goes away.
  afterAll(() => {
    for (const handle of pending) cancel(handle);
    pending.clear();
  });
}
