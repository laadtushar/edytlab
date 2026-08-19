/**
 * Micro-interactions (#211 phase 2).
 *
 * Phase 2 is about direct manipulation *feeling* direct — the feedback
 * that says a control is under your pointer and is being held, rather
 * than that a value changed some time after you let go.
 *
 * Two of these are stylesheet guards for the same reason the vocabulary
 * ones are: a hover rule that quietly stops existing is invisible to
 * every other kind of test, and the focus ring in particular is the one
 * whose absence nobody notices until they try to use the app without a
 * mouse.
 */

import { fireEvent, render, screen } from "@testing-library/react";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

import { ClipStrip } from "../ClipStrip";
import type { ClipSummary } from "../../lib/tauri-bridge";

const css = readFileSync(join(process.cwd(), "src", "styles.css"), "utf8");

if (css.trim().length === 0) {
  throw new Error("styles.css read as empty — the guards below would be vacuous");
}

describe("focus is visible", () => {
  it("uses the focus ring token that was defined and never referenced", () => {
    // `--ring-focus` sat in the token block unused while the app had
    // zero occurrences of focus-visible anywhere, so a keyboard user
    // got the webview default against a near-black background.
    expect(css).toContain(":focus-visible");
    expect(css).toMatch(/:focus-visible\s*\{[^}]*var\(--ring-focus\)/);
  });

  it("hangs the ring off :focus-visible, not :focus", () => {
    // `:focus` would leave a ring behind after every mouse click, which
    // is the thing that makes people delete focus styling altogether.
    //
    // Checked by looking for a bare `:focus` *selector* — a `{` after
    // it, with only selector characters in between. Comments talk
    // about `:focus` in prose and must not count; an earlier version of
    // this test sliced from the first occurrence of ":focus-visible",
    // found the one in the comment above the rule, and failed on the
    // word "`:focus`" in that comment rather than on any CSS.
    const withoutComments = css.replace(/\/\*[\s\S]*?\*\//g, "");
    expect(withoutComments).not.toMatch(/:focus(?!-visible)[^{;]*\{/);
  });
});

describe("faders acknowledge being grabbed", () => {
  it("changes the cursor while held", () => {
    expect(css).toMatch(/input\[type="range"\]\s*\{[^}]*cursor:\s*grab/);
    expect(css).toMatch(
      /input\[type="range"\]:active\s*\{[^}]*cursor:\s*grabbing/,
    );
  });

  it("grows the thumb on hover and again on press", () => {
    // Small on purpose — a mixing control dragged hundreds of times
    // should not become a thing that moves while you aim at it.
    expect(css).toMatch(/:hover::-webkit-slider-thumb\s*\{[^}]*scale\(1\.15\)/);
    expect(css).toMatch(/:active::-webkit-slider-thumb\s*\{[^}]*scale\(1\.3\)/);
  });

  it("times the thumb with the vocabulary rather than a literal", () => {
    expect(css).toMatch(
      /::-webkit-slider-thumb\s*\{[^}]*transition:[^;]*var\(--dur-1\)/,
    );
  });
});

describe("a node arriving in the graph", () => {
  it("scales from 0.94 rather than from nothing", () => {
    // A node growing from zero draws the eye to the growing. The
    // *position* is the information in this view — which parent it
    // hangs off is the entire point — so the entry must not compete
    // with where it landed.
    expect(css).toMatch(/@keyframes node-in[\s\S]*?scale\(0\.94\)/);
  });

  it("does not translate", () => {
    const block = css.slice(css.indexOf("@keyframes node-in"));
    const body = block.slice(0, block.indexOf("\n}"));
    expect(body).not.toContain("translate");
  });
});

describe("the clip chip says when it is being held", () => {
  const clips = [
    { source_path: "/a/one.wav", start_sec: 0, length_sec: 10 },
    { source_path: "/a/two.wav", start_sec: 10, length_sec: 10 },
  ] as ClipSummary[];

  function strip() {
    return render(
      <ClipStrip
        clips={clips}
        duration={100}
        selectedClip={null}
        onSelectClip={() => {}}
        onMoveClip={() => {}}
        onRemoveClip={() => {}}
        trackName="voice"
      />,
    );
  }

  it("is grab at rest and grabbing while dragged", () => {
    // It said `grab` throughout, so the one moment the pointer was
    // actually holding something looked exactly like hovering over it.
    strip();
    const chip = screen.getByTestId("clip-chip-0");
    expect(chip.style.cursor).toBe("grab");

    fireEvent.pointerDown(chip, { clientX: 100 });
    expect(screen.getByTestId("clip-chip-0").style.cursor).toBe("grabbing");

    fireEvent.pointerUp(window, { clientX: 100 });
    expect(screen.getByTestId("clip-chip-0").style.cursor).toBe("grab");
  });

  it("leaves the other chips alone", () => {
    strip();
    fireEvent.pointerDown(screen.getByTestId("clip-chip-0"), { clientX: 100 });
    expect(screen.getByTestId("clip-chip-1").style.cursor).toBe("grab");
  });
});
