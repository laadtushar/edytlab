/**
 * Clip motion (#211 phase 3).
 *
 * The audit called this the biggest one in the app: a cut, a split or a
 * paste rearranges the strip, and until now every chip teleported, so
 * the only way to know what an edit did was to remember where things
 * had been.
 *
 * jsdom computes no animations, so these are not tests that motion
 * *happened* — a test claiming that would be observing nothing and
 * passing regardless. They test the two decisions that make the motion
 * right or wrong, both of which are ordinary state and are exactly what
 * a later refactor would break without noticing:
 *
 *   1. the chip under the pointer does not ease, and
 *   2. nothing eases on the very first paint.
 */

import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { ClipStrip } from "../ClipStrip";
import type { ClipSummary } from "../../lib/tauri-bridge";

function clip(start: number, length: number, name = "take.wav"): ClipSummary {
  return {
    source_path: `/audio/${name}`,
    start_sec: start,
    length_sec: length,
  } as ClipSummary;
}

const TWO = [clip(0, 10), clip(10, 10, "second.wav")];

function renderStrip(clips: ClipSummary[] = TWO) {
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

describe("clips travel to their new positions", () => {
  it("does not animate on the first paint", () => {
    // Otherwise every chip slides in from the left edge on load, and
    // again on every track switch — a loading flourish on the surface
    // that most needs to look settled.
    renderStrip();
    expect(screen.getByTestId("clip-chip-0").dataset.motion).toBe("none");
    expect(screen.getByTestId("clip-chip-1").dataset.motion).toBe("none");
  });

  it("animates once the strip has been painted and the clips change", () => {
    const { rerender } = renderStrip();

    // An edit that shortens the first clip shifts the second one left.
    // That shift is the thing being explained.
    rerender(
      <ClipStrip
        clips={[clip(0, 5), clip(5, 10, "second.wav")]}
        duration={100}
        selectedClip={null}
        onSelectClip={() => {}}
        onMoveClip={() => {}}
        onRemoveClip={() => {}}
        trackName="voice"
      />,
    );

    expect(screen.getByTestId("clip-chip-1").dataset.motion).toBe(
      "clip-travel",
    );
  });

  it("does not ease the chip being dragged", () => {
    // The dragged chip already follows the pointer frame by frame.
    // Easing it as well makes it trail the cursor and overshoot on
    // release — a direct-manipulation gesture that stops feeling
    // direct, which is worse than the teleporting this change fixes.
    const { rerender } = renderStrip();
    rerender(
      <ClipStrip
        clips={TWO}
        duration={100}
        selectedClip={null}
        onSelectClip={() => {}}
        onMoveClip={() => {}}
        onRemoveClip={() => {}}
        trackName="voice"
      />,
    );

    const chip = screen.getByTestId("clip-chip-0");
    expect(chip.dataset.motion).toBe("clip-travel");

    fireEvent.pointerDown(chip, { clientX: 100 });

    expect(screen.getByTestId("clip-chip-0").dataset.motion).toBe("none");
    // ...and only that one. The others are being rearranged around it
    // and should still explain themselves.
    expect(screen.getByTestId("clip-chip-1").dataset.motion).toBe(
      "clip-travel",
    );
  });

  it("eases again once the drag is released", () => {
    const { rerender } = renderStrip();
    rerender(
      <ClipStrip
        clips={TWO}
        duration={100}
        selectedClip={null}
        onSelectClip={() => {}}
        onMoveClip={() => {}}
        onRemoveClip={() => {}}
        trackName="voice"
      />,
    );

    const chip = screen.getByTestId("clip-chip-0");
    fireEvent.pointerDown(chip, { clientX: 100 });
    expect(screen.getByTestId("clip-chip-0").dataset.motion).toBe("none");

    fireEvent.pointerUp(window, { clientX: 100 });
    expect(screen.getByTestId("clip-chip-0").dataset.motion).toBe(
      "clip-travel",
    );
  });
});
