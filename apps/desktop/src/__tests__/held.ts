/**
 * A promise the test settles when it chooses — inside `act`.
 *
 * For a mocked backend call whose answer a test needs to control in
 * time: to act while the call is still in flight, or to assert that an
 * answer was *ignored*. For the second, waiting until the mock "has been
 * called" proves nothing — the answer arrives after that, and React draws
 * what it does with it later still. An assertion in between passes
 * whether or not the component ignored anything, and on a slow runner it
 * always is in between.
 *
 * Settling inside `act` makes React finish with the answer before the
 * returned promise resolves, so the next line sees the result, however
 * slow the scheduler.
 */

import { act } from "@testing-library/react";

export interface Held<T> {
  /** Hand this to the mock: `mock.mockReturnValue(read.promise)`. */
  promise: Promise<T>;
  /** Answer with `value`, and let React finish with it. */
  resolve(value: T): Promise<void>;
  /** Fail with `reason`, and let React finish with it. */
  reject(reason: unknown): Promise<void>;
}

export function held<T>(): Held<T> {
  let settleOk!: (value: T) => void;
  let settleErr!: (reason: unknown) => void;
  const promise = new Promise<T>((resolve, reject) => {
    settleOk = resolve;
    settleErr = reject;
  });
  // Awaited below so `act` returns only once the answer has landed. It
  // also marks the promise handled, which means vitest will not report a
  // rejection the component drops: a test that rejects must assert what
  // the component does about it, not rely on that report.
  const settled = promise.catch(() => undefined);
  return {
    promise,
    resolve: (value) =>
      act(async () => {
        settleOk(value);
        await settled;
      }),
    reject: (reason) =>
      act(async () => {
        settleErr(reason);
        await settled;
      }),
  };
}
