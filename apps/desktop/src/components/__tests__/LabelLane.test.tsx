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

  it("adds a label at the point you double-clicked", () => {
    const h = setup();
    fireEvent.doubleClick(screen.getByTestId("label-lane-track"), { clientX: 250 });
    expect(h.onAdd).toHaveBeenCalledTimes(1);
    expect(h.onAdd.mock.calls[0][0]).toBeCloseTo(25, 5);
  });

  /**
   * A single click must not create one — that is how you deselect, and
   * a label per stray click would be maddening.
   */
  it("does not add on a single click", () => {
    const h = setup();
    fireEvent.click(screen.getByTestId("label-lane-track"), { clientX: 250 });
    expect(h.onAdd).not.toHaveBeenCalled();
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

  /** Before any audio is loaded the lane is a header and nothing else. */
  it("draws no chips without a duration", () => {
    setup({ duration: 0 });
    expect(screen.queryAllByTestId("label-chip")).toHaveLength(0);
  });
});
