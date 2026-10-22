"use client";

import dynamic from "next/dynamic";
import { useEffect, useState } from "react";

/**
 * The scroll-progress bar and the press/hover feedback, loaded after the
 * page has painted instead of with it.
 *
 * Both are GSAP: the first drives a `scrub` ScrollTrigger over the whole
 * page, the second wires `matchMedia` and a set of delegated pointer
 * listeners. They were mounted by the root layout, so every page — a
 * privacy policy as much as the home page — built them during
 * hydration, in the same long task that makes the page interactive.
 * Neither is needed for the first paint or for reading: the bar is one
 * pixel of decoration and the feedback is a nudge on a control that is
 * already usable. A reader cannot press anything in the first idle
 * moment after the page appears, so they start then.
 *
 * Each is a separate `dynamic` import, `ssr: false`, so neither is in
 * the entry chunk and neither is in the server's HTML (the bar is
 * `aria-hidden` and starts at zero width, so there is nothing to lose).
 * `requestIdleCallback` waits for the main thread to be free after
 * hydration; the `timeout` makes sure a busy page still gets them, and
 * Safari, which has no `requestIdleCallback`, takes a short timer.
 */
const ScrollProgress = dynamic(() => import("./scroll-progress").then((m) => m.ScrollProgress), {
  ssr: false,
});
const MicroInteractions = dynamic(() => import("./micro-interactions").then((m) => m.MicroInteractions), {
  ssr: false,
});

export function SiteMotion() {
  const [ready, setReady] = useState(false);

  useEffect(() => {
    const start = () => setReady(true);
    if (typeof window.requestIdleCallback === "function") {
      const id = window.requestIdleCallback(start, { timeout: 3000 });
      return () => window.cancelIdleCallback(id);
    }
    const id = window.setTimeout(start, 400);
    return () => window.clearTimeout(id);
  }, []);

  if (!ready) return null;
  return (
    <>
      <ScrollProgress />
      <MicroInteractions />
    </>
  );
}
