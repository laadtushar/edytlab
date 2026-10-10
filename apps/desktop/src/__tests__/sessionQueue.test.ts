/**
 * The session's line (#498): one turn at a time, in the order queued, and
 * a turn that fails does not stop the ones behind it.
 *
 * `App.orderedEdits.test.tsx` drives the same thing through the real app;
 * these pin the line itself, with no React and no backend in the way.
 */

import { describe, expect, it } from "vitest";

import { createSessionQueue } from "../lib/sessionQueue";

/** A promise the test settles when it chooses. */
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((ok, fail) => {
    resolve = ok;
    reject = fail;
  });
  return { promise, resolve, reject };
}

/** Let every promise reaction already queued run. */
const flush = () => new Promise<void>((resolve) => setTimeout(resolve, 0));

describe("createSessionQueue", () => {
  it("starts a turn only when the one before it has finished", async () => {
    const { run } = createSessionQueue();
    const first = deferred<string>();
    const log: string[] = [];

    const a = run(async () => {
      log.push("a starts");
      const v = await first.promise;
      log.push("a ends");
      return v;
    });
    const b = run(async () => {
      log.push("b starts");
      return "b";
    });

    await flush();
    expect(log).toEqual(["a starts"]);

    first.resolve("a");
    expect(await a).toBe("a");
    expect(await b).toBe("b");
    expect(log).toEqual(["a starts", "a ends", "b starts"]);
  });

  it("runs turns in the order they were queued, whichever would finish first", async () => {
    const { run } = createSessionQueue();
    const slow = deferred<void>();
    const order: number[] = [];

    const turns = [
      run(async () => {
        await slow.promise;
        order.push(1);
      }),
      run(async () => {
        order.push(2);
      }),
      run(async () => {
        order.push(3);
      }),
    ];
    await flush();
    expect(order).toEqual([]);

    slow.resolve();
    await Promise.all(turns);
    expect(order).toEqual([1, 2, 3]);
  });

  it("hands the failure to whoever queued the turn, and still runs the next", async () => {
    const { run } = createSessionQueue();
    const first = deferred<string>();

    const failing = run(() => first.promise);
    const next = run(async () => "next");
    // Settled before anything awaits it: no unhandled rejection either.
    const failed = expect(failing).rejects.toBe("refused");

    first.reject("refused");
    await failed;
    expect(await next).toBe("next");
  });

  it("is not stopped by a turn that throws before it returns a promise", async () => {
    const { run } = createSessionQueue();

    const throwing = run((() => {
      throw new Error("threw");
    }) as () => Promise<void>);
    await expect(throwing).rejects.toThrow("threw");

    expect(await run(async () => "still running")).toBe("still running");
  });

  it("keeps working after any number of failures", async () => {
    const { run } = createSessionQueue();
    for (let i = 0; i < 5; i++) {
      await expect(run(() => Promise.reject(new Error(`no ${i}`)))).rejects.toThrow(`no ${i}`);
    }
    expect(await run(async () => 5)).toBe(5);
  });

  it("is one line per queue: another queue is not held up by it", async () => {
    const one = createSessionQueue();
    const other = createSessionQueue();
    const blocked = deferred<void>();

    const stuck = one.run(() => blocked.promise);
    expect(await other.run(async () => "free")).toBe("free");

    blocked.resolve();
    await stuck;
  });
});
