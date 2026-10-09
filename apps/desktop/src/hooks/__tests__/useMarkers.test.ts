/**
 * useMarkers (#453): the label lane's list follows the head.
 *
 * The lane re-read on `marker-changed` only, and `set_head_to` emits no
 * such event, so an undo that took back a label left the chip on screen
 * and a redo that brought it back left the lane empty. The hook reads on
 * every head move as well.
 *
 * The state under test is what the hook returns, so each case waits for
 * it, or settles a held reply inside `act` (`__tests__/held.ts`), and
 * never for a mock to have been called: that holds on a slow scheduler
 * too (`pnpm test:slow-scheduler`).
 */

import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { held } from "../../__tests__/held";
import type { Marker } from "../../lib/tauri-bridge";

const bridge = vi.hoisted(() => ({
  listMarkers: vi.fn(),
  onMarkerChanged: vi.fn(),
  unlisten: vi.fn(),
  /** The callback the hook handed to `onMarkerChanged`. */
  changed: null as (() => void) | null,
}));

vi.mock("../../lib/tauri-bridge", () => ({
  listMarkers: bridge.listMarkers,
  onMarkerChanged: bridge.onMarkerChanged,
}));

import { useMarkers } from "../useMarkers";

const label = (name: string, timeSec = 1): Marker => ({
  id: `id-${name}`,
  name,
  kind: "marker",
  time_sec: timeSec,
});

const verse = label("verse");
const chorus = label("chorus", 2);

function mount(head: string | null) {
  return renderHook(({ head: h }: { head: string | null }) => useMarkers(h), {
    initialProps: { head },
  });
}

beforeEach(() => {
  bridge.changed = null;
  bridge.listMarkers.mockReset();
  bridge.unlisten.mockReset();
  bridge.onMarkerChanged.mockReset().mockImplementation((cb: () => void) => {
    bridge.changed = cb;
    return Promise.resolve(bridge.unlisten);
  });
});

describe("useMarkers", () => {
  it("reads once at mount and shows the reply", async () => {
    bridge.listMarkers.mockResolvedValue([verse]);
    const { result } = mount("A");
    expect(result.current).toEqual([]);

    await waitFor(() => expect(result.current).toEqual([verse]));
    expect(bridge.listMarkers).toHaveBeenCalledTimes(1);
  });

  it("re-reads when the head moves, and shows the new head's labels", async () => {
    // Undo took the label back: the head moved, nothing else was said.
    bridge.listMarkers.mockResolvedValueOnce([verse]).mockResolvedValueOnce([]);
    const { result, rerender } = mount("B");
    await waitFor(() => expect(result.current).toEqual([verse]));

    rerender({ head: "A" });
    await waitFor(() => expect(result.current).toEqual([]));

    // And redo brought it back.
    bridge.listMarkers.mockResolvedValueOnce([verse]);
    rerender({ head: "B" });
    await waitFor(() => expect(result.current).toEqual([verse]));
    expect(bridge.listMarkers).toHaveBeenCalledTimes(3);
  });

  it("re-reads when a marker-changed event arrives", async () => {
    bridge.listMarkers.mockResolvedValueOnce([verse]).mockResolvedValueOnce([verse, chorus]);
    const { result } = mount("A");
    await waitFor(() => expect(result.current).toEqual([verse]));

    expect(bridge.changed).not.toBeNull();
    act(() => bridge.changed!());
    await waitFor(() => expect(result.current).toEqual([verse, chorus]));
  });

  it("keeps the newest reply when an older one arrives last", async () => {
    // A head move while the first read is still in flight: the second
    // read is the newer question, and its answer is the one that counts.
    const first = held<Marker[]>();
    const second = held<Marker[]>();
    bridge.listMarkers.mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise);
    const { result, rerender } = mount("A");
    rerender({ head: "B" });

    await second.resolve([chorus]);
    expect(result.current).toEqual([chorus]);

    await first.resolve([verse]);
    expect(result.current).toEqual([chorus]);
  });

  it("ignores an older failure once a newer read has been made", async () => {
    const first = held<Marker[]>();
    const second = held<Marker[]>();
    bridge.listMarkers.mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise);
    const { result, rerender } = mount("A");
    rerender({ head: "B" });

    await second.resolve([chorus]);
    await first.reject("no session loaded");
    expect(result.current).toEqual([chorus]);
  });

  it("empties the list when the read is refused", async () => {
    // `NoSession`: a project with nothing in it yet.
    bridge.listMarkers.mockResolvedValueOnce([verse]);
    const { result, rerender } = mount("A");
    await waitFor(() => expect(result.current).toEqual([verse]));

    const refused = held<Marker[]>();
    bridge.listMarkers.mockReturnValueOnce(refused.promise);
    rerender({ head: "B" });
    await refused.reject("no session loaded; call open_project first");
    expect(result.current).toEqual([]);
  });

  it("does not read again for the same head", async () => {
    bridge.listMarkers.mockResolvedValue([verse]);
    const { result, rerender } = mount("A");
    await waitFor(() => expect(result.current).toEqual([verse]));
    expect(bridge.listMarkers).toHaveBeenCalledTimes(1);

    // Effects have run by the time `rerender` returns: nothing is pending.
    rerender({ head: "A" });
    expect(bridge.listMarkers).toHaveBeenCalledTimes(1);
  });

  it("stops listening when it unmounts", async () => {
    const listening = held<() => void>();
    bridge.onMarkerChanged.mockReturnValue(listening.promise);
    bridge.listMarkers.mockResolvedValue([]);
    const { unmount } = mount("A");
    await listening.resolve(bridge.unlisten);

    unmount();
    expect(bridge.unlisten).toHaveBeenCalledTimes(1);
  });

  it("stops listening even when it unmounts before the subscription is made", async () => {
    // `listen` is asynchronous: cleanup can run before it resolves, and
    // the listener that arrives afterwards must be detached at once.
    const listening = held<() => void>();
    bridge.onMarkerChanged.mockReturnValue(listening.promise);
    bridge.listMarkers.mockResolvedValue([]);
    const { unmount } = mount("A");

    unmount();
    expect(bridge.unlisten).not.toHaveBeenCalled();
    await listening.resolve(bridge.unlisten);
    expect(bridge.unlisten).toHaveBeenCalledTimes(1);
  });

  it("still shows the labels when the subscription is refused", async () => {
    const listening = held<() => void>();
    bridge.onMarkerChanged.mockReturnValue(listening.promise);
    bridge.listMarkers.mockResolvedValue([verse]);
    const { result } = mount("A");

    await listening.reject("no event bridge");
    await waitFor(() => expect(result.current).toEqual([verse]));
  });
});
