/**
 * The motion system (#211).
 *
 * These are guards on the two things that rot silently.
 *
 * The vocabulary rots by omission: the point of naming three durations
 * is that the fourth one nobody names does not appear, and the way that
 * fails is a component quietly hard-coding `200ms` because it was
 * quicker than looking the token up. A stylesheet test is the only place
 * that catches it, since nothing about a hard-coded duration is a type
 * error or a lint failure.
 *
 * Reduced motion rots the same way, worse: the person it fails is the
 * least likely to be in the room when it is missed, and there is no
 * symptom in anyone else's session.
 */

import { act, render, screen } from "@testing-library/react";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it, vi } from "vitest";

import { LEAVE_MS } from "../../hooks/usePresence";
import { CommandPalette } from "../CommandPalette";
import { ErrorBanner } from "../ErrorBanner";
import { KeyringWaiting } from "../KeyringWaiting";
import { ShortcutsOverlay } from "../ShortcutsOverlay";
import { TemplatePickerModal } from "../TemplatePickerModal";

// Read from disk rather than importing the stylesheet.
//
// `import css from "../../styles.css?raw"` is the obvious way to do
// this and silently yields the empty string: the vitest config sets
// `css: false`, which stubs CSS imports out, and `?raw` does not
// escape that. Every assertion below would then run against "".
//
// `process.cwd()` rather than `import.meta.url`, which is not a file:
// URL once Vite has transformed the module.
//
// Line endings are normalised because a Windows checkout has CRLF, and
// the reduced-motion block below is found by its `\n}\n` ending.
const readSource = (...path: string[]) =>
  readFileSync(join(process.cwd(), "src", ...path), "utf8").replace(/\r\n/g, "\n");
const css = readSource("styles.css");

// Fail loudly on an empty read. Without this, the whole suite below
// degrades into asserting things about "" — which is the exact failure
// the `?raw` attempt produced, and the kind that reads as a passing
// guard once someone writes the assertions slightly differently.
if (css.trim().length === 0) {
  throw new Error("styles.css read as empty — the guards below would be vacuous");
}

describe("the motion vocabulary", () => {
  it("names three durations and two easings", () => {
    for (const token of ["--dur-1", "--dur-2", "--dur-3"]) {
      expect(css).toContain(`${token}:`);
    }
    for (const token of ["--ease-out", "--ease-in-out"]) {
      expect(css).toContain(`${token}:`);
    }
  });

  it("routes a bare Tailwind `transition` through the vocabulary", () => {
    // Thirteen components write bare `transition`. If this default is
    // ever removed they all silently revert to Tailwind's 150ms, and
    // nothing else in the suite would notice.
    expect(css).toContain("--default-transition-duration: 120ms");
    expect(css).toContain("--default-transition-timing-function:");
  });

  it("defines every keyframe class in terms of the tokens", () => {
    // The failure this catches is a new animation shipping with a
    // literal duration, which is how a vocabulary becomes decoration.
    const animationDecls = css.match(/animation:[^;]+;/g) ?? [];
    expect(animationDecls.length).toBeGreaterThan(0);
    for (const decl of animationDecls) {
      expect(decl).toMatch(/var\(--dur-\d\)/);
    }
  });
});

describe("reduced motion", () => {
  it("is honoured globally rather than per-component", () => {
    expect(css).toContain("@media (prefers-reduced-motion: reduce)");
  });

  it("neutralises animations without stopping the event that fires", () => {
    // 0.01ms rather than 0: `animationend` still fires, so anything
    // sequencing off it (`usePresence`'s fast path) completes instead of
    // waiting forever. Setting this to 0 is the plausible "cleanup" that
    // would turn reduced motion into a hang.
    const block = css.slice(
      css.indexOf("@media (prefers-reduced-motion: reduce)"),
    );
    expect(block).toContain("animation-duration: 0.01ms !important");
    expect(block).not.toMatch(/animation-duration:\s*0s?\s*!important/);
  });

  it("switches transitions off rather than shortening them", () => {
    // `transition-duration: 0.01ms` on every element makes every property
    // change on every element a transition (the default `transition-property`
    // is `all`), and a transition holds the old value until the next frame.
    // Layout read in the same tick then sees the old box: zoom-to-selection
    // assigned the timeline scrollbar's `scrollLeft` after its content's
    // width changed, and the assignment was clamped to the old range.
    // `e2e/zoom.spec.ts` and `timeline-axis.spec.ts` fail under reduced
    // motion if this goes back.
    const block = css.slice(
      css.indexOf("@media (prefers-reduced-motion: reduce)"),
    );
    const reduced = /@media \(prefers-reduced-motion: reduce\) \{[\s\S]*?\n\}\n/.exec(block)![0];
    expect(reduced).toContain("transition: none !important");
    expect(reduced).not.toMatch(/transition-duration:/);
  });
});

describe("overlays arrive rather than flashing", () => {
  // All three render `null` when closed, so without an entry animation
  // they appear between two frames with nothing in between. The assert
  // is on the class the animation hangs off, because jsdom computes no
  // animations — a test claiming to observe the motion itself would be
  // observing nothing and passing regardless.

  it("the command palette animates its panel and its backdrop", () => {
    const { container } = render(
      <CommandPalette open onClose={() => {}} onSelect={() => {}} />,
    );
    expect(container.querySelector(".backdrop-in")).not.toBeNull();
    expect(container.querySelector(".overlay-in")).not.toBeNull();
  });

  it("the shortcuts sheet animates its panel and its backdrop", () => {
    render(<ShortcutsOverlay open onClose={() => {}} />);
    const overlay = screen.getByTestId("shortcuts-overlay");
    expect(overlay.className).toContain("backdrop-in");
    expect(overlay.querySelector(".overlay-in")).not.toBeNull();
  });

  it("the template picker animates its panel and its backdrop", () => {
    const { container } = render(
      <TemplatePickerModal
        open
        onClose={() => {}}
        templates={[]}
        onSelect={() => {}}
      />,
    );
    expect(container.querySelector(".backdrop-in")).not.toBeNull();
    expect(container.querySelector(".overlay-in")).not.toBeNull();
  });

  it("still renders nothing at all when closed", () => {
    // The animation classes must not have turned "closed" into
    // "present but transparent" — an invisible overlay still traps
    // clicks and still takes focus.
    const { container } = render(
      <CommandPalette open={false} onClose={() => {}} onSelect={() => {}} />,
    );
    expect(container.firstChild).toBeNull();
  });
});

describe("overlays leave rather than blinking out", () => {
  // The other half of #211, which shipped only the arrive side while
  // `docs/motion-audit.md` recorded both as done (#236).
  //
  // Each surface renders `null` the instant its flag flips, so an exit
  // animation is impossible from CSS alone — `animation-fill-mode:
  // both` retains an *entry* animation's last frame and has nothing to
  // say about an element about to stop existing. `usePresence` holds
  // the mount open; these assert that it does, and that what is held
  // carries the exit class rather than the entry one.
  //
  // As above, the assertion is on the class, not on observed motion:
  // jsdom computes no animations, so a test claiming to watch the fade
  // would pass against no fade at all.

  it("the command palette holds its mount and swaps to the exit classes", () => {
    const { container, rerender } = render(
      <CommandPalette open onClose={() => {}} onSelect={() => {}} />,
    );
    expect(container.querySelector(".overlay-in")).not.toBeNull();

    rerender(
      <CommandPalette open={false} onClose={() => {}} onSelect={() => {}} />,
    );
    // Still present — this is the whole point.
    expect(container.firstChild).not.toBeNull();
    expect(container.querySelector(".backdrop-out")).not.toBeNull();
    expect(container.querySelector(".overlay-out")).not.toBeNull();
    // And not still claiming to be arriving.
    expect(container.querySelector(".overlay-in")).toBeNull();
  });

  it("the shortcuts sheet holds its mount and swaps to the exit classes", () => {
    const { rerender } = render(<ShortcutsOverlay open onClose={() => {}} />);
    rerender(<ShortcutsOverlay open={false} onClose={() => {}} />);
    const overlay = screen.getByTestId("shortcuts-overlay");
    expect(overlay.className).toContain("backdrop-out");
    expect(overlay.querySelector(".overlay-out")).not.toBeNull();
  });

  it("the template picker holds its mount and swaps to the exit classes", () => {
    const props = {
      templates: [],
      onSelect: () => {},
      onClose: () => {},
    };
    const { container, rerender } = render(
      <TemplatePickerModal open {...props} />,
    );
    rerender(<TemplatePickerModal open={false} {...props} />);
    expect(container.querySelector(".backdrop-out")).not.toBeNull();
    expect(container.querySelector(".overlay-out")).not.toBeNull();
  });

  /**
   * The exit has to actually end. A hold with no release is a worse
   * bug than the blink it replaces — the overlay would sit on screen
   * forever, trapping clicks and focus.
   *
   * jsdom fires no `animationend`, so this exercises the timer path
   * specifically, which is the one that has to work when the animation
   * does not run.
   */
  it("unmounts once the leave is over", async () => {
    vi.useFakeTimers();
    try {
      const { container, rerender } = render(
        <CommandPalette open onClose={() => {}} onSelect={() => {}} />,
      );
      rerender(
        <CommandPalette open={false} onClose={() => {}} onSelect={() => {}} />,
      );
      expect(container.firstChild).not.toBeNull();

      await act(async () => {
        vi.advanceTimersByTime(LEAVE_MS * 4);
      });
      expect(container.firstChild).toBeNull();
    } finally {
      vi.useRealTimers();
    }
  });

  /**
   * Re-opening mid-leave must cancel it. Otherwise the timer from the
   * close fires afterwards and unmounts the overlay the user just
   * reopened — which looks like the app ignoring the second press.
   */
  it("cancels a leave when reopened before it finishes", async () => {
    vi.useFakeTimers();
    try {
      const { container, rerender } = render(
        <CommandPalette open onClose={() => {}} onSelect={() => {}} />,
      );
      rerender(
        <CommandPalette open={false} onClose={() => {}} onSelect={() => {}} />,
      );
      rerender(
        <CommandPalette open onClose={() => {}} onSelect={() => {}} />,
      );

      await act(async () => {
        vi.advanceTimersByTime(LEAVE_MS * 4);
      });
      expect(container.firstChild).not.toBeNull();
      expect(container.querySelector(".overlay-in")).not.toBeNull();
      expect(container.querySelector(".overlay-out")).toBeNull();
    } finally {
      vi.useRealTimers();
    }
  });

  /**
   * The hold is keyed to the stylesheet's own token. If `--dur-2` is
   * retuned and `LEAVE_MS` is not, the unmount either truncates the
   * animation or leaves a dead element on screen after it — and
   * neither is a type error.
   */
  it("holds for exactly one --dur-2", () => {
    expect(css).toContain(`--dur-2: ${LEAVE_MS}ms`);
  });

  /**
   * Every exit class the components apply must exist in the
   * stylesheet. A renamed keyframe would otherwise mean the hold
   * happens with nothing drawn during it — a pause instead of a fade,
   * which is worse than the blink.
   */
  it("defines every exit animation the components ask for", () => {
    for (const name of ["overlay-out", "backdrop-out", "strip-out"]) {
      expect(css).toContain(`@keyframes ${name}`);
      expect(css).toContain(`.${name} {`);
    }
  });
});

// ---------------------------------------------------------------------------
// Phase 3: the site's feel, in CSS.
//
// Everything here is a stylesheet or class-presence guard. jsdom computes
// no animation and these tests apply no stylesheet, so none of them can
// claim that motion *happened*; that is `e2e/motion.spec.ts`, in a real
// browser. What can rot unseen, and is pinned here, is the vocabulary, the
// reduced-motion contract, and the list of things that must never move.
// ---------------------------------------------------------------------------

/** CSS with comments removed, so prose about a token is not read as the token. */
const code = css.replace(/\/\*[\s\S]*?\*\//g, "");

/** The body of the braces opened at `open` (the index of a `{`). */
function braceBody(source: string, open: number): string {
  let depth = 0;
  for (let i = open; i < source.length; i++) {
    if (source[i] === "{") depth++;
    else if (source[i] === "}" && --depth === 0) return source.slice(open + 1, i);
  }
  throw new Error("unbalanced braces");
}

/** Every `@keyframes` block: its name, and the properties its frames set. */
function keyframes(): Map<string, Set<string>> {
  const found = new Map<string, Set<string>>();
  for (const m of code.matchAll(/@keyframes\s+([\w-]+)\s*\{/g)) {
    const body = braceBody(code, m.index! + m[0].length - 1);
    const props = new Set<string>();
    // Declarations only: `name: value`, not the `from` / `to` / `50%` frames.
    for (const d of body.matchAll(/[{;]\s*([a-z-]+)\s*:/g)) props.add(d[1]);
    for (const d of body.matchAll(/^\s*([a-z-]+)\s*:/gm)) props.add(d[1]);
    found.set(m[1], props);
  }
  return found;
}

describe("the motion vocabulary, phase 3", () => {
  it("adds exactly two tokens, a spring easing and a stagger gap", () => {
    expect(css).toMatch(/--ease-spring:\s*cubic-bezier\(0\.34,\s*1\.56,\s*0\.64,\s*1\)/);
    expect(css).toMatch(/--stagger:\s*30ms/);
  });

  it("keeps the explaining duration out of entrances", () => {
    // --dur-3 is for a change the eye must follow (a clip travelling, a
    // curve drawing on). Nothing that merely arrives uses it.
    const users = [...code.matchAll(/([^{}]+)\{[^{}]*var\(--dur-3\)[^{}]*\}/g)].map((m) =>
      m[1].trim(),
    );
    expect(users).toEqual([".automation-draw-on"]);
  });

  it("retimes the app-wide fade to an entrance", () => {
    expect(code).toMatch(/\.app-fade-in\s*\{[^}]*animation:[^;]*var\(--dur-2\)/);
  });

  it("deletes the infinite empty-state pulse, keyframes included", () => {
    expect(css).not.toContain("wave-pulse");
  });

  it.each([
    ["fade-in", "fade-in", "--dur-2"],
    ["fade-in-quick", "fade-in", "--dur-1"],
    ["plan-in", "plan-in", "--dur-2"],
    ["stagger-in", "stagger-in", "--dur-1"],
    ["reveal-in", "stagger-in", "--dur-2"],
    ["badge-settle", "badge-settle", "--dur-2"],
    ["banner-in", "banner-in", "--dur-2"],
  ])(".%s plays %s over %s, once", (cls, frames, dur) => {
    expect(css).toContain(`@keyframes ${frames}`);
    const rule = new RegExp(`\\.${cls}\\s*\\{([^}]*)\\}`).exec(code);
    expect(rule, `no .${cls} rule`).not.toBeNull();
    expect(rule![1]).toMatch(new RegExp(`animation:\\s*${frames}\\s+var\\(${dur}\\)`));
    // `backwards`: the first frame holds through a delay, so a staggered
    // item is not visible early. Not `both`: the last frame is the
    // element's own style, and a forwards fill would keep the animation in
    // effect for good and leave the element rendering differently from
    // one that never animated (measured; see styles.css).
    expect(rule![1]).toMatch(/\bbackwards\b/);
    expect(rule![1]).not.toMatch(/\b(both|forwards)\b/);
    expect(rule![1]).not.toMatch(/infinite/);
  });

  it("caps every stagger, so a long list is not a long wait", () => {
    // 5 steps of 30ms for the plan and chips (150ms + 120ms = 270ms); 3 for
    // the empty state's groups (90ms + 200ms = 290ms). Both under 300.
    expect(code).toMatch(
      /\.stagger-in\s*\{[^}]*animation-delay:\s*calc\(min\(var\(--i,\s*0\),\s*5\)\s*\*\s*var\(--stagger\)\)/,
    );
    expect(code).toMatch(
      /\.reveal-in\s*\{[^}]*animation-delay:\s*calc\(min\(var\(--i,\s*0\),\s*3\)\s*\*\s*var\(--stagger\)\)/,
    );
  });

  it("builds the logo once: bars with a spring, strokes by dash offset", () => {
    expect(css).toContain("@keyframes logo-bar-in");
    expect(css).toContain("@keyframes logo-line-draw");
    expect(code).toMatch(/\.logo-build \[data-logo-bar\]\s*\{[^}]*var\(--ease-spring\)/);
    expect(code).toMatch(/\.logo-build \[data-logo-line\]\s*\{[^}]*stroke-dasharray:\s*1/);
    expect(code).not.toMatch(/\.logo-build[^{]*\{[^}]*infinite/);
  });
});

describe("motion stays cheap and bounded", () => {
  const CHEAP = new Set([
    "opacity",
    "transform",
    "translate",
    "scale",
    "stroke-dashoffset",
    // Older keyframes, kept as they are: the curve's draw-on (a clip-path
    // sweep) and the progress strip (#211, #236).
    "clip-path",
    "grid-template-rows",
  ]);

  it("finds the keyframes it is checking", () => {
    // A parser that found nothing would make the next two vacuous.
    const names = [...keyframes().keys()];
    for (const n of ["app-fade-in", "overlay-in", "plan-in", "stagger-in", "logo-bar-in", "logo-line-draw", "strip-in"]) {
      expect(names).toContain(n);
    }
    expect(keyframes().get("plan-in")).toEqual(new Set(["opacity", "translate"]));
    expect(keyframes().get("logo-bar-in")).toEqual(new Set(["transform"]));
    expect(keyframes().get("strip-in")).toEqual(new Set(["grid-template-rows", "opacity"]));
  });

  it("animates nothing but opacity, transform, scale, translate and the strokes", () => {
    for (const [name, props] of keyframes()) {
      for (const prop of props) {
        expect(CHEAP.has(prop), `@keyframes ${name} animates ${prop}`).toBe(true);
      }
    }
  });

  it("never animates a filter, or a layout property", () => {
    for (const [name, props] of keyframes()) {
      for (const prop of props) {
        expect(prop, `@keyframes ${name}`).not.toMatch(
          /^(filter|backdrop-filter|width|height|top|left|right|bottom|margin|padding)/,
        );
      }
    }
    // And no backdrop blur anywhere in the sheet.
    expect(code).not.toContain("backdrop-filter");
  });

  it("has no infinite animation of its own", () => {
    // Tailwind's `animate-spin` / `animate-pulse` are for something busy;
    // the stylesheet defines none, and the idle screens use none.
    expect(code).not.toMatch(/animation:[^;]*infinite/);
  });
});

describe("the press", () => {
  // Declared only where motion is welcome. Under reduced motion the rule
  // is absent, so the global block never leaves a 0.96 that snaps on and
  // off: a state would otherwise survive the flattening.
  const at = code.indexOf("@media (prefers-reduced-motion: no-preference)");
  const inside = braceBody(code, code.indexOf("{", at));

  it("is inside a no-preference block, and only there", () => {
    expect(at).toBeGreaterThan(-1);
    expect(inside).toMatch(/:active\s*\{\s*scale:\s*0\.96/);
    const outside = code.slice(0, at) + code.slice(code.indexOf("{", at) + inside.length + 2);
    expect(outside).not.toMatch(/scale:\s*0\.96/);
  });

  it("covers the recipes, pills and [data-press], and not a disabled button", () => {
    for (const sel of [
      ".btn-primary",
      ".btn-secondary",
      ".btn-tool",
      ".btn-ghost",
      ".btn-danger",
      "button.pill",
      "[data-press]",
    ]) {
      expect(inside).toContain(sel);
    }
    expect(inside).toContain(":not(:disabled):active");
  });

  it("uses the individual `scale` property, which composes with a transform", () => {
    expect(inside).not.toMatch(/transform\s*:/);
  });

  it("excludes drag handles and ranges by never naming them", () => {
    for (const handle of ["range", "chip", "automation", "react-flow", "label", "slider"]) {
      expect(inside).not.toContain(handle);
    }
  });

  it("eases in the recipes, in --dur-1, with scale in the list", () => {
    expect(code).toMatch(
      /\.btn-primary,[\s\S]*?\.ring-hover\s*\{[^}]*transition-property:[^;]*\bscale\b[^;]*;[^}]*transition-duration:\s*var\(--dur-1\)/,
    );
  });

  it("glows a little wider on the primary action under the pointer", () => {
    expect(code).toMatch(
      /\.btn-primary:hover:not\(:disabled\)\s*\{[^}]*box-shadow:\s*var\(--shadow-accent-hover\)/,
    );
    expect(css).toMatch(/--shadow-accent-hover:/);
  });
});

describe("links and tabs", () => {
  it("sweeps a link's underline in from the left and out to the right", () => {
    expect(code).toMatch(
      /\.link-accent::after\s*\{[^}]*scale:\s*0 1[^}]*transform-origin:\s*right[^}]*transition:\s*scale var\(--dur-2\)/,
    );
    expect(code).toMatch(
      /\.link-accent:hover::after,\s*\.link-accent:focus-visible::after\s*\{[^}]*scale:\s*1 1[^}]*transform-origin:\s*left/,
    );
    // The abrupt text-decoration underline it replaced.
    expect(code).not.toMatch(/\.link-accent:hover\s*\{[^}]*text-decoration/);
  });

  it("slides a tab's indicator in from the left, without resizing the tab", () => {
    const rule = /\.tab-indicator::after\s*\{([^}]*)\}/.exec(code);
    expect(rule).not.toBeNull();
    expect(rule![1]).toMatch(/transform-origin:\s*left/);
    expect(rule![1]).toMatch(/transition:\s*scale var\(--dur-2\)/);
    // A pseudo-element's scale: no width, height, padding or weight moves.
    expect(rule![1]).not.toMatch(/transition:[^;]*(width|height|padding|font-weight)/);
  });
});

describe("reduced motion, phase 3", () => {
  const block = css.slice(css.indexOf("@media (prefers-reduced-motion: reduce)"));

  it("drops animation delays as well as durations", () => {
    // A staggered reveal flattened to 0.01ms would still hold each item
    // back and pop them in one at a time.
    expect(block).toContain("animation-delay: 0s !important");
  });

  it("is a single global block, not a list of components", () => {
    expect(css.match(/@media \(prefers-reduced-motion: reduce\)/g)).toHaveLength(1);
  });
});

describe("banners drop in", () => {
  it("the error banner hangs from the edge above", () => {
    const { container } = render(<ErrorBanner message="Oops" testId="render-error" />);
    expect(screen.getByTestId("render-error").className).toContain("banner-in");
    expect(container.querySelector(".app-fade-in")).toBeNull();
  });

  it("the keyring notice does too", async () => {
    render(<KeyringWaiting pending delayMs={0} />);
    const notice = await screen.findByTestId("keychain-waiting");
    expect(notice.className).toContain("banner-in");
  });
});

describe("never animate what WaveSurfer draws, or anything above it", () => {
  // These are source guards, like brandTokens.test.ts: the ancestors of a
  // lane's canvases are in App.tsx and Timeline.tsx, and a class on one of
  // them is not a type error. `e2e/motion.spec.ts` checks the same thing
  // against computed style in a real browser.
  const app = readSource("App.tsx");
  const timeline = readSource("components", "Timeline.tsx");
  const MOTION_CLASS =
    /\b(app-fade-in|fade-in|fade-in-quick|plan-in|stagger-in|reveal-in|banner-in|badge-settle|overlay-in|backdrop-in|node-in|strip-in|logo-build)\b/;

  it("<main> carries no entry animation", () => {
    const main = /<main\s[^>]*>/.exec(app);
    expect(main).not.toBeNull();
    expect(main![0]).not.toMatch(MOTION_CLASS);
  });

  it("timeline-root carries no entry animation", () => {
    const root = /data-testid="timeline-root"[\s\S]{0,400}?style=\{\{/.exec(timeline);
    expect(root).not.toBeNull();
    // The comment that says why is allowed to name the class.
    expect(root![0].replace(/\/\/[^\n]*/g, "")).not.toMatch(MOTION_CLASS);
  });

  it("a lane, its surface and its waveform container carry no class at all", () => {
    for (const id of ["timeline-lane-waveform", "timeline-lane-surface", "timeline-lane"]) {
      const at = timeline.indexOf(`data-testid="${id}"`);
      expect(at, id).toBeGreaterThan(-1);
      const tag = timeline.slice(timeline.lastIndexOf("<div", at), timeline.indexOf("style=", at));
      expect(tag, id).not.toMatch(/className=/);
    }
  });

  it("no lane is keyed by anything that would remount it to animate it", () => {
    // `key={...}` on a TrackLane remounts its WaveSurfer; the leaving lane
    // is the shape of the #455 freeze. Lanes are keyed by track identity.
    const lanes = [...timeline.matchAll(/<TrackLane\b[\s\S]*?\/>/g)].map((m) => m[0]);
    expect(lanes.length).toBeGreaterThan(0);
    for (const lane of lanes) {
      expect(lane).not.toMatch(/key=\{[^}]*(tab|view|fade|anim|enter|leav|presence|Date|random)/i);
    }
  });

  it("adds no media element for a UI effect", () => {
    // The only <audio> / <video> in the components are the audition player
    // and the timeline's mix players, both there before this pass.
    const files = ["Settings", "Chat", "EmptyState", "ToolBadge", "MessageBubble", "ErrorBanner", "AppHeader"];
    for (const f of files) {
      const src = readSource("components", `${f}.tsx`);
      expect(src, f).not.toMatch(/<(audio|video)\b|new Audio\(/);
    }
  });
});
