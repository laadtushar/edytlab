/**
 * Whether the user has asked their OS to reduce motion.
 *
 * The CSS side of this is handled globally in styles.css, and that
 * covers every transition and keyframe in the app. This hook is for the
 * cases CSS cannot reach: motion driven from JavaScript, and — more
 * importantly — the decision *not to run* an effect at all. Neutralising
 * a duration still runs the effect; sometimes the right answer is to
 * skip it and jump to the end state.
 *
 * It subscribes rather than reading once. Someone who turns the setting
 * on mid-session is very likely doing it *because* of what they are
 * currently looking at, and making them restart the app to be listened
 * to would be a poor answer to that.
 */

import { useEffect, useState } from "react";

const QUERY = "(prefers-reduced-motion: reduce)";

/**
 * Read the current value without subscribing.
 *
 * Guarded because `matchMedia` is absent in some test environments and
 * in SSR. Absent means "no preference expressed", which is the same
 * answer as a user who has not set it — so the app animates, which is
 * the existing behaviour rather than a new one.
 */
export function prefersReducedMotion(): boolean {
  if (typeof window === "undefined" || !window.matchMedia) return false;
  return window.matchMedia(QUERY).matches;
}

export function useReducedMotion(): boolean {
  const [reduced, setReduced] = useState(prefersReducedMotion);

  useEffect(() => {
    if (typeof window === "undefined" || !window.matchMedia) return;
    const mq = window.matchMedia(QUERY);

    // Re-read on mount as well as subscribing: the initial useState ran
    // during render, and the setting can change between that and the
    // effect firing.
    setReduced(mq.matches);

    const onChange = (e: MediaQueryListEvent) => setReduced(e.matches);

    // Safari below 14 has no addEventListener on MediaQueryList, and
    // Tauri uses the host webview — WKWebView on macOS, WebKitGTK on
    // Linux — so the old addListener path is a real fallback here
    // rather than a theoretical one.
    if (mq.addEventListener) {
      mq.addEventListener("change", onChange);
      return () => mq.removeEventListener("change", onChange);
    }
    mq.addListener(onChange);
    return () => mq.removeListener(onChange);
  }, []);

  return reduced;
}
