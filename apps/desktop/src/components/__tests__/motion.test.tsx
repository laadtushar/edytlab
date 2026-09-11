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
const css = readFileSync(join(process.cwd(), "src", "styles.css"), "utf8");

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

  it("neutralises durations without stopping the events that fire", () => {
    // 0.01ms rather than 0: `transitionend` and `animationend` still
    // fire, so anything sequencing off them completes instead of
    // waiting forever. Setting this to 0 is the plausible "cleanup"
    // that would turn reduced motion into a hang.
    const block = css.slice(
      css.indexOf("@media (prefers-reduced-motion: reduce)"),
    );
    expect(block).toContain("animation-duration: 0.01ms !important");
    expect(block).toContain("transition-duration: 0.01ms !important");
    expect(block).not.toMatch(/animation-duration:\s*0s?\s*!important/);
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
