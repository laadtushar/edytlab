/**
 * useHeadMove (#373): one undo or redo at a time, and a word when one
 * is slow — but never for an ordinary one.
 */

import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { HEAD_MOVE_PENDING_DELAY_MS, useHeadMove } from "../useHeadMove";

/** A move the test finishes when it chooses. */
function held<T>() {
  let finish!: (value: T) => void;
  const done = new Promise<T>((resolve) => {
    finish = resolve;
  });
  return { done, finish };
}

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
});

describe("useHeadMove", () => {
  it("waits long enough that an undo answered from disk shows nothing", () => {
    // A rebuild takes seconds; an ordinary undo, a few milliseconds.
    expect(HEAD_MOVE_PENDING_DELAY_MS).toBeGreaterThanOrEqual(100);
  });

  it("says nothing about a move that answers inside the delay", async () => {
    const { result } = renderHook(() => useHeadMove());
    const move = held<string>();

    let ran!: Promise<string | undefined>;
    act(() => {
      ran = result.current.run(() => move.done);
    });
    act(() => {
      vi.advanceTimersByTime(HEAD_MOVE_PENDING_DELAY_MS - 1);
    });
    expect(result.current.pending).toBe(false);
    await act(async () => {
      move.finish("moved");
      await ran;
    });
    act(() => {
      vi.advanceTimersByTime(HEAD_MOVE_PENDING_DELAY_MS * 10);
    });

    expect(result.current.pending).toBe(false);
    await expect(ran).resolves.toBe("moved");
  });

  it("says a slow move is pending, and stops once it lands", async () => {
    const { result } = renderHook(() => useHeadMove());
    const move = held<void>();

    let ran!: Promise<void | undefined>;
    act(() => {
      ran = result.current.run(() => move.done);
    });
    act(() => {
      vi.advanceTimersByTime(HEAD_MOVE_PENDING_DELAY_MS);
    });
    expect(result.current.pending).toBe(true);

    await act(async () => {
      move.finish();
      await ran;
    });
    expect(result.current.pending).toBe(false);
  });

  it("ignores a second move while one is in flight", async () => {
    const { result } = renderHook(() => useHeadMove());
    const first = held<void>();
    const second = vi.fn(async () => "second");

    let ran!: Promise<void | undefined>;
    act(() => {
      ran = result.current.run(() => first.done);
    });
    let ignored: string | undefined = "not run";
    await act(async () => {
      ignored = await result.current.run(second);
    });
    expect(second).not.toHaveBeenCalled();
    expect(ignored).toBeUndefined();

    await act(async () => {
      first.finish();
      await ran;
    });
    // Once it lands, the next one runs.
    await act(async () => {
      await result.current.run(second);
    });
    expect(second).toHaveBeenCalledTimes(1);
  });

  it("is free again after a move that fails", async () => {
    const { result } = renderHook(() => useHeadMove());
    await act(async () => {
      await expect(
        result.current.run(async () => {
          throw new Error("cannot rebuild take.wav");
        }),
      ).rejects.toThrow("cannot rebuild");
    });
    expect(result.current.pending).toBe(false);

    const next = vi.fn(async () => undefined);
    await act(async () => {
      await result.current.run(next);
    });
    expect(next).toHaveBeenCalledTimes(1);
  });
});
