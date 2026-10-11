/**
 * The label lane (#203 §1).
 *
 * The acceptance is four verbs — add, rename, move, delete — and the
 * tests are written as those verbs rather than as component internals,
 * because that is what the ticket promises and what will still be true
 * if the lane is rebuilt.
 */

import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { LabelLane } from "../LabelLane";
import type { Marker } from "../../lib/tauri-bridge";

const DURATION = 100;
const WIDTH = 1000;

const LABELS: Marker[] = [
  { id: "a", name: "intro", kind: "marker", time_sec: 10 },
  { id: "b", name: "chapter two", kind: "marker", time_sec: 50 },
];

/** The lane track measures itself; jsdom gives everything zero width. */
function pinWidth() {
  Object.defineProperty(HTMLElement.prototype, "getBoundingClientRect", {
    configurable: true,
    value: () => ({
      left: 0,
      top: 0,
      right: WIDTH,
      bottom: 28,
      width: WIDTH,
      height: 28,
      x: 0,
      y: 0,
      toJSON: () => ({}),
    }),
  });
}

function setup(overrides: Partial<React.ComponentProps<typeof LabelLane>> = {}) {
  pinWidth();
  const handlers = {
    onAdd: vi.fn(),
    onRename: vi.fn(),
    onMove: vi.fn(),
    onRemove: vi.fn(),
    onSeek: vi.fn(),
  };
  render(
    <LabelLane labels={LABELS} duration={DURATION} sidebarWidth={0} {...handlers} {...overrides} />,
  );
  return handlers;
}

describe("the label lane", () => {
  it("shows the labels the session already has", () => {
    setup();
    expect(screen.getAllByTestId("label-chip")).toHaveLength(2);
    expect(screen.getByText("intro")).toBeTruthy();
    expect(screen.getByText("chapter two")).toBeTruthy();
  });

  /** Position is the whole point: a label at 10s of 100s sits at 10%. */
  it("puts each label where its time says", () => {
    setup();
    const chips = screen.getAllByTestId("label-chip");
    expect(chips[0].style.left).toBe("10%");
    expect(chips[1].style.left).toBe("50%");
  });

  /**
   * The name is typed in the lane, next to the audio. It used to come
   * from `window.prompt`, which a macOS webview answers with `null`
   * (WKWebView only asks the host when the host implements the prompt
   * panel, and neither wry nor the dialog plugin does), so there the
   * double-click did nothing at all.
   */
  it("opens a box at the point you double-clicked, and adds the label you type", () => {
    const h = setup();
    fireEvent.doubleClick(screen.getByTestId("label-lane-track"), { clientX: 250 });
    expect(h.onAdd).not.toHaveBeenCalled();

    const input = screen.getByTestId("label-draft-input") as HTMLInputElement;
    expect(input).toHaveFocus();
    fireEvent.change(input, { target: { value: "  cold open " } });
    fireEvent.keyDown(input, { key: "Enter" });

    expect(h.onAdd).toHaveBeenCalledTimes(1);
    expect(h.onAdd.mock.calls[0][0]).toBeCloseTo(25, 5);
    expect(h.onAdd.mock.calls[0][1]).toBe("cold open");
    expect(screen.queryByTestId("label-draft-input")).toBeNull();
  });

  it("puts the box where the label will go", () => {
    setup();
    fireEvent.doubleClick(screen.getByTestId("label-lane-track"), { clientX: 250 });
    expect(screen.getByTestId("label-draft").style.left).toBe("25%");
  });

  /** Near the right edge the lane would clip the box and hide what is typed. */
  it("opens the box to the left of the flag when there is no room to its right", () => {
    setup();
    fireEvent.doubleClick(screen.getByTestId("label-lane-track"), { clientX: 950 });
    expect(screen.getByTestId("label-draft").style.transform).toContain("-100%");
    expect(screen.getByTestId("label-draft").style.left).toBe("95%");

    fireEvent.keyDown(screen.getByTestId("label-draft-input"), { key: "Escape" });
    fireEvent.doubleClick(screen.getByTestId("label-lane-track"), { clientX: 250 });
    expect(screen.getByTestId("label-draft").style.transform).not.toContain("-100%");
  });

  it("adds nothing when the box is left empty, or abandoned with Escape", () => {
    const h = setup();
    fireEvent.doubleClick(screen.getByTestId("label-lane-track"), { clientX: 250 });
    fireEvent.keyDown(screen.getByTestId("label-draft-input"), { key: "Enter" });
    expect(h.onAdd).not.toHaveBeenCalled();
    expect(screen.queryByTestId("label-draft-input")).toBeNull();

    fireEvent.doubleClick(screen.getByTestId("label-lane-track"), { clientX: 250 });
    const input = screen.getByTestId("label-draft-input");
    fireEvent.change(input, { target: { value: "nope" } });
    fireEvent.keyDown(input, { key: "Escape" });
    expect(h.onAdd).not.toHaveBeenCalled();
    expect(screen.queryByTestId("label-draft-input")).toBeNull();
  });

  /** Clicking away from a named box keeps the label, as it does for a rename. */
  it("adds the label when focus leaves a box with a name in it", () => {
    const h = setup();
    fireEvent.doubleClick(screen.getByTestId("label-lane-track"), { clientX: 250 });
    const input = screen.getByTestId("label-draft-input");
    fireEvent.change(input, { target: { value: "outro" } });
    fireEvent.blur(input);
    expect(h.onAdd).toHaveBeenCalledTimes(1);
    expect(h.onAdd.mock.calls[0][1]).toBe("outro");
  });

  /** Enter and the blur that follows it are one gesture, not two labels. */
  it("adds once when Enter is followed by the box losing focus", () => {
    const h = setup();
    fireEvent.doubleClick(screen.getByTestId("label-lane-track"), { clientX: 250 });
    const input = screen.getByTestId("label-draft-input");
    fireEvent.change(input, { target: { value: "outro" } });
    fireEvent.keyDown(input, { key: "Enter" });
    fireEvent.blur(input);
    expect(h.onAdd).toHaveBeenCalledTimes(1);
  });

  /**
   * A single click must not create one — that is how you deselect, and
   * a label per stray click would be maddening.
   */
  it("does not open a box on a single click", () => {
    const h = setup();
    fireEvent.click(screen.getByTestId("label-lane-track"), { clientX: 250 });
    expect(screen.queryByTestId("label-draft-input")).toBeNull();
    expect(h.onAdd).not.toHaveBeenCalled();
  });

  /** Double-clicking a chip renames it; it is not an empty spot to add at. */
  it("does not open a box on a double-click over a chip", () => {
    setup();
    fireEvent.doubleClick(screen.getAllByTestId("label-chip-button")[0]);
    expect(screen.queryByTestId("label-draft-input")).toBeNull();
  });

  it("renames on double-click, type, Enter", () => {
    const h = setup();
    fireEvent.doubleClick(screen.getAllByTestId("label-chip-button")[0]);
    const input = screen.getByTestId("label-chip-input") as HTMLInputElement;
    fireEvent.change(input, { target: { value: "cold open" } });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(h.onRename).toHaveBeenCalledWith("a", "cold open");
  });

  /** Escape has to be a real way out, or a mistype is unrecoverable. */
  it("abandons a rename on Escape", () => {
    const h = setup();
    fireEvent.doubleClick(screen.getAllByTestId("label-chip-button")[0]);
    const input = screen.getByTestId("label-chip-input") as HTMLInputElement;
    fireEvent.change(input, { target: { value: "nope" } });
    fireEvent.keyDown(input, { key: "Escape" });
    expect(h.onRename).not.toHaveBeenCalled();
    expect(screen.getByText("intro")).toBeTruthy();
  });

  it("does not rename when the text is unchanged", () => {
    const h = setup();
    fireEvent.doubleClick(screen.getAllByTestId("label-chip-button")[0]);
    fireEvent.keyDown(screen.getByTestId("label-chip-input"), { key: "Enter" });
    expect(h.onRename).not.toHaveBeenCalled();
  });

  it("moves a label to where the drag ended", () => {
    const h = setup();
    fireEvent.mouseDown(screen.getAllByTestId("label-chip-button")[0], {
      button: 0,
      clientX: 100,
    });
    fireEvent.mouseMove(window, { clientX: 700 });
    fireEvent.mouseUp(window, { clientX: 700 });
    expect(h.onMove).toHaveBeenCalledTimes(1);
    expect(h.onMove.mock.calls[0][0]).toBe("a");
    expect(h.onMove.mock.calls[0][1]).toBeCloseTo(70, 5);
  });

  /**
   * The chip is a button wider than the flag it hangs from, so a press on
   * it lands some way along from the label's own time. A drag moves the
   * label by how far the pointer moved, not to where the pointer is: the
   * label used to jump to the pointer's time, which is not where it was
   * grabbed.
   */
  it("moves a label by how far it was dragged, not to where the pointer is", () => {
    const h = setup();
    const chip = screen.getAllByTestId("label-chip-button")[0];
    // The label is at 10s (100px); the chip is grabbed 30px along.
    fireEvent.mouseDown(chip, { button: 0, clientX: 130 });
    fireEvent.mouseMove(window, { clientX: 730 });
    fireEvent.mouseUp(window, { clientX: 730 });
    expect(h.onMove).toHaveBeenCalledTimes(1);
    expect(h.onMove.mock.calls[0][0]).toBe("a");
    expect(h.onMove.mock.calls[0][1]).toBeCloseTo(70, 5);
  });

  /** The drag is shown where it will land, not under the pointer. */
  it("shows the chip where the drag will land while it is held", () => {
    setup();
    const chip = screen.getAllByTestId("label-chip-button")[0];
    fireEvent.mouseDown(chip, { button: 0, clientX: 130 });
    fireEvent.mouseMove(window, { clientX: 730 });
    expect(screen.getAllByTestId("label-chip")[0].style.left).toBe("70%");
  });

  /**
   * A click on a chip away from its flag is a click. It used to move the
   * label to the time under the pointer, so selecting a chip, or the two
   * clicks of a double-click to rename it, made the label jump and cost
   * an undo step each.
   */
  it("does not move a label when its chip is clicked away from the flag", () => {
    const h = setup();
    const chip = screen.getAllByTestId("label-chip-button")[0];
    fireEvent.mouseDown(chip, { button: 0, clientX: 130 });
    fireEvent.mouseUp(window, { clientX: 130 });
    fireEvent.click(chip);
    expect(h.onMove).not.toHaveBeenCalled();
    expect(h.onSeek).toHaveBeenCalledWith(10);
  });

  it("does not move a label to rename it", () => {
    const h = setup();
    const chip = screen.getAllByTestId("label-chip-button")[0];
    for (let i = 0; i < 2; i++) {
      fireEvent.mouseDown(chip, { button: 0, clientX: 130 });
      fireEvent.mouseUp(window, { clientX: 130 });
      fireEvent.click(chip);
    }
    fireEvent.doubleClick(chip);
    fireEvent.change(screen.getByTestId("label-chip-input"), { target: { value: "cold open" } });
    fireEvent.keyDown(screen.getByTestId("label-chip-input"), { key: "Enter" });
    expect(h.onMove).not.toHaveBeenCalled();
    expect(h.onRename).toHaveBeenCalledWith("a", "cold open");
  });

  /** A hand does not hold still: a press that wobbles a pixel or two is a click. */
  it("treats a press that moved only a pixel or two as a click", () => {
    const h = setup();
    const chip = screen.getAllByTestId("label-chip-button")[0];
    fireEvent.mouseDown(chip, { button: 0, clientX: 130 });
    fireEvent.mouseMove(window, { clientX: 132 });
    fireEvent.mouseUp(window, { clientX: 132 });
    expect(h.onMove).not.toHaveBeenCalled();
  });

  /**
   * The click that follows a drag's release is not a request to seek:
   * it would put the playhead at the label's old position.
   */
  it("does not seek when a drag is released over the chip", () => {
    const h = setup();
    const chip = screen.getAllByTestId("label-chip-button")[0];
    fireEvent.mouseDown(chip, { button: 0, clientX: 130 });
    fireEvent.mouseMove(window, { clientX: 730 });
    fireEvent.mouseUp(window, { clientX: 730 });
    fireEvent.click(chip);
    expect(h.onSeek).not.toHaveBeenCalled();
  });

  /**
   * Pressing and releasing without moving is a click. Committing it as
   * a move would cost an undo step for looking at something.
   */
  it("does not move when the drag went nowhere", () => {
    const h = setup();
    const chip = screen.getAllByTestId("label-chip-button")[0];
    fireEvent.mouseDown(chip, { button: 0, clientX: 100 });
    fireEvent.mouseUp(window, { clientX: 100 });
    expect(h.onMove).not.toHaveBeenCalled();
  });

  it("deletes on right-click", () => {
    const h = setup();
    fireEvent.contextMenu(screen.getAllByTestId("label-chip-button")[1]);
    expect(h.onRemove).toHaveBeenCalledWith("b");
  });

  it("seeks when a label is clicked", () => {
    const h = setup();
    fireEvent.click(screen.getAllByTestId("label-chip-button")[1]);
    expect(h.onSeek).toHaveBeenCalledWith(50);
  });

  /** Region labels sit at their start; the lane must not assume markers. */
  it("places a region label at its start", () => {
    setup({
      labels: [{ id: "r", name: "verse", kind: "region", start_sec: 20, end_sec: 40 }],
    });
    expect(screen.getByTestId("label-chip").style.left).toBe("20%");
  });

  /**
   * A region label has an extent, and the lane shows it: a bar from its
   * start to its end under the chip. A marker has none.
   */
  it("draws the extent of a region label, and none for a marker", () => {
    setup({
      labels: [
        { id: "r", name: "verse", kind: "region", start_sec: 20, end_sec: 45 },
        { id: "m", name: "cue", kind: "marker", time_sec: 60 },
      ],
    });
    const bars = screen.getAllByTestId("label-extent");
    expect(bars).toHaveLength(1);
    expect(bars[0].style.left).toBe("20%");
    expect(bars[0].style.width).toBe("25%");
  });

  /** Zoomed in, the bar is clipped by the lane like everything else in it. */
  it("draws the extent on the lane's own axis when zoomed", () => {
    setup({
      labels: [{ id: "r", name: "verse", kind: "region", start_sec: 20, end_sec: 40 }],
      view: { start: 10, end: 50 },
    });
    const bar = screen.getByTestId("label-extent");
    expect(bar.style.left).toBe("25%");
    expect(bar.style.width).toBe("50%");
  });

  /** Before any audio is loaded the lane is a header and nothing else. */
  it("draws no chips without a duration", () => {
    setup({ duration: 0 });
    expect(screen.queryAllByTestId("label-chip")).toHaveLength(0);
  });
});
