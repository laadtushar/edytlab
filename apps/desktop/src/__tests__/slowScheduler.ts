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

if (import.meta.env.MODE === "slow-scheduler") {
  const later = globalThis.setTimeout;
  (globalThis as unknown as { setImmediate: unknown }).setImmediate = (
    fn: (...args: unknown[]) => void,
    ...args: unknown[]
  ) => later(fn, 40, ...args);
}

export {};
