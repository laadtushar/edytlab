/**
 * The per-track dropdown (#161).
 *
 * Every action here already existed as a tool; what was missing was a
 * way to reach one without typing a sentence to the agent. So the tests
 * are about the wiring being real — the right *session* track index,
 * the rename actually committing — and about the menu not becoming a
 * trap.
 */

import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { TrackMenu } from "../components/TrackMenu";

function setup(trackIndex = 2, trackName = "bass") {
  const onRename = vi.fn();
  const onDuplicate = vi.fn();
  const onRemove = vi.fn();
  render(
    <TrackMenu
      trackIndex={trackIndex}
      trackName={trackName}
      onRename={onRename}
      onDuplicate={onDuplicate}
      onRemove={onRemove}
    />,
  );
  return { onRename, onDuplicate, onRemove, trackIndex };
}

describe("TrackMenu", () => {
  it("is closed until asked for", () => {
    setup();
    expect(screen.queryByTestId("track-menu-2")).toBeNull();
    expect(screen.getByTestId("track-menu-btn-2")).toHaveAttribute(
      "aria-expanded",
      "false",
    );
  });

  it("addresses the session track index, not the lane position", () => {
    const { onDuplicate, onRemove } = setup(2);
    fireEvent.click(screen.getByTestId("track-menu-btn-2"));

    fireEvent.click(screen.getByTestId("track-duplicate-2"));
    expect(onDuplicate).toHaveBeenCalledWith(2);

    fireEvent.click(screen.getByTestId("track-menu-btn-2"));
    fireEvent.click(screen.getByTestId("track-remove-2"));
    expect(onRemove).toHaveBeenCalledWith(2);
  });

  it("renames on Enter", () => {
    const { onRename } = setup(2, "bass");
    fireEvent.click(screen.getByTestId("track-menu-btn-2"));
    fireEvent.click(screen.getByTestId("track-rename-2"));

    const input = screen.getByTestId("track-rename-input-2");
    fireEvent.change(input, { target: { value: "bass DI" } });
    fireEvent.keyDown(input, { key: "Enter" });

    expect(onRename).toHaveBeenCalledWith(2, "bass DI");
    expect(screen.queryByTestId("track-menu-2")).toBeNull();
  });

  /**
   * An empty name would leave a lane head with nothing on it, which
   * reads as a rendering bug rather than as a choice.
   */
  it("refuses an empty name rather than committing it", () => {
    const { onRename } = setup(2, "bass");
    fireEvent.click(screen.getByTestId("track-menu-btn-2"));
    fireEvent.click(screen.getByTestId("track-rename-2"));

    const input = screen.getByTestId("track-rename-input-2");
    fireEvent.change(input, { target: { value: "   " } });
    fireEvent.keyDown(input, { key: "Enter" });

    expect(onRename).not.toHaveBeenCalled();
  });

  it("does not fire a rename that changes nothing", () => {
    const { onRename } = setup(2, "bass");
    fireEvent.click(screen.getByTestId("track-menu-btn-2"));
    fireEvent.click(screen.getByTestId("track-rename-2"));
    fireEvent.keyDown(screen.getByTestId("track-rename-input-2"), {
      key: "Enter",
    });
    expect(onRename).not.toHaveBeenCalled();
  });

  /**
   * The lane below listens for bare keys — L toggles loop, space plays,
   * arrows seek. Typing a track name must not do any of that.
   */
  it("keeps typing out of the transport shortcuts", () => {
    const onWindowKey = vi.fn();
    window.addEventListener("keydown", onWindowKey);
    setup(2, "bass");
    fireEvent.click(screen.getByTestId("track-menu-btn-2"));
    fireEvent.click(screen.getByTestId("track-rename-2"));
    onWindowKey.mockClear();

    fireEvent.keyDown(screen.getByTestId("track-rename-input-2"), { key: "l" });
    expect(onWindowKey).not.toHaveBeenCalled();
    window.removeEventListener("keydown", onWindowKey);
  });

  it("closes on Escape and on a click outside", () => {
    setup(2);
    fireEvent.click(screen.getByTestId("track-menu-btn-2"));
    expect(screen.getByTestId("track-menu-2")).toBeTruthy();
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByTestId("track-menu-2")).toBeNull();

    fireEvent.click(screen.getByTestId("track-menu-btn-2"));
    expect(screen.getByTestId("track-menu-2")).toBeTruthy();
    fireEvent.mouseDown(document.body);
    expect(screen.queryByTestId("track-menu-2")).toBeNull();
  });
});
