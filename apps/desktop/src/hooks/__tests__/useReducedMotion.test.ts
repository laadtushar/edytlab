/**
 * useReducedMotion (#211).
 *
 * The CSS handles every transition and keyframe. This hook exists for
 * the decision CSS cannot express: whether to run an effect at all. So
 * the tests are about it reporting the truth, and about it keeping up
 * when the truth changes mid-session.
 */

import { act, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { prefersReducedMotion, useReducedMotion } from "../useReducedMotion";

type Listener = (e: MediaQueryListEvent) => void;

/** Install a controllable matchMedia; returns a setter for the value. */
function installMatchMedia(initial: boolean, legacy = false) {
  const listeners: Listener[] = [];
  let matches = initial;

  const mq = {
    get matches() {
      return matches;
    },
    media: "(prefers-reduced-motion: reduce)",
    addEventListener: legacy
      ? undefined
      : (_: string, cb: Listener) => listeners.push(cb),
    removeEventListener: legacy
      ? undefined
      : (_: string, cb: Listener) => {
          const i = listeners.indexOf(cb);
          if (i >= 0) listeners.splice(i, 1);
        },
    addListener: (cb: Listener) => listeners.push(cb),
    removeListener: (cb: Listener) => {
      const i = listeners.indexOf(cb);
      if (i >= 0) listeners.splice(i, 1);
    },
  };

  vi.stubGlobal(
    "matchMedia",
    vi.fn(() => mq),
  );

  return {
    set(next: boolean) {
      matches = next;
      for (const cb of [...listeners]) {
        cb({ matches: next } as MediaQueryListEvent);
      }
    },
    listenerCount: () => listeners.length,
  };
}

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("useReducedMotion", () => {
  it("reports the preference when it is set", () => {
    installMatchMedia(true);
    const { result } = renderHook(() => useReducedMotion());
    expect(result.current).toBe(true);
  });

  it("reports no preference as false, so the app animates", () => {
    installMatchMedia(false);
    const { result } = renderHook(() => useReducedMotion());
    expect(result.current).toBe(false);
  });

  it("keeps up when the setting changes mid-session", () => {
    // Someone turning this on while the app is open is very likely
    // doing it *because* of what they are looking at. Making them
    // restart to be listened to would be a poor answer to that.
    const mq = installMatchMedia(false);
    const { result } = renderHook(() => useReducedMotion());
    expect(result.current).toBe(false);

    act(() => mq.set(true));
    expect(result.current).toBe(true);

    act(() => mq.set(false));
    expect(result.current).toBe(false);
  });

  it("unsubscribes on unmount", () => {
    const mq = installMatchMedia(false);
    const { unmount } = renderHook(() => useReducedMotion());
    expect(mq.listenerCount()).toBe(1);
    unmount();
    expect(mq.listenerCount()).toBe(0);
  });

  it("falls back to addListener where addEventListener is absent", () => {
    // Tauri runs on the host webview — WKWebView on macOS, WebKitGTK on
    // Linux — so this old path is a real fallback rather than a
    // theoretical one.
    const mq = installMatchMedia(false, true);
    const { result } = renderHook(() => useReducedMotion());
    act(() => mq.set(true));
    expect(result.current).toBe(true);
  });

  it("treats a missing matchMedia as no preference", () => {
    // Absent means the question was never asked, which is the same
    // answer as a user who has not set it — so the app behaves exactly
    // as it did before this hook existed, rather than silently
    // disabling motion everywhere.
    vi.stubGlobal("matchMedia", undefined);
    expect(prefersReducedMotion()).toBe(false);
    const { result } = renderHook(() => useReducedMotion());
    expect(result.current).toBe(false);
  });
});
