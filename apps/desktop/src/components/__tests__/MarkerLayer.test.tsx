import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { MarkerLayer } from "../MarkerLayer";
import type { Marker } from "../../lib/tauri-bridge";

const mk = (id: string, name: string, time: number): Marker => ({
  id,
  name,
  kind: "marker",
  time_sec: time,
});

describe("MarkerLayer", () => {
  it("renders one flag per marker", () => {
    const markers = [mk("1", "chorus", 10), mk("2", "drop", 20)];
    render(
      <MarkerLayer markers={markers} duration={30} onSeek={() => {}} onRemove={() => {}} />,
    );
    expect(screen.getAllByTestId("marker-flag")).toHaveLength(2);
  });

  it("clicking a flag calls onSeek with marker time", () => {
    const onSeek = vi.fn();
    render(
      <MarkerLayer
        markers={[mk("1", "chorus", 10)]}
        duration={30}
        onSeek={onSeek}
        onRemove={() => {}}
      />,
    );
    // The interactive element is the button inside the marker-flag container.
    fireEvent.click(screen.getByTitle(/Seek to chorus/));
    expect(onSeek).toHaveBeenCalledWith(10);
  });

  it("right-clicking a flag calls onRemove", () => {
    const onRemove = vi.fn();
    render(
      <MarkerLayer
        markers={[mk("1", "chorus", 10)]}
        duration={30}
        onSeek={() => {}}
        onRemove={onRemove}
      />,
    );
    // Right-click (contextMenu) on the button inside the flag.
    fireEvent.contextMenu(screen.getByTitle(/Seek to chorus/));
    expect(onRemove).toHaveBeenCalledWith("1");
  });

  it("renders nothing when duration is 0", () => {
    render(
      <MarkerLayer markers={[mk("1", "chorus", 10)]} duration={0} onSeek={() => {}} onRemove={() => {}} />,
    );
    expect(screen.queryByTestId("marker-flag")).toBeNull();
  });
});
