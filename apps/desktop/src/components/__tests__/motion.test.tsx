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

import { render, screen } from "@testing-library/react";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

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
