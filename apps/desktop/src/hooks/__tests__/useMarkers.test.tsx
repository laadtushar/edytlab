/**
 * useMarkers (#453): the label lane's list is the session's `markers` query.
 *
 * What makes it read again is not the hook's: `useSession` invalidates the
 * query when a head moves (undo took a label back, redo brought it back, an
 * agent edit, another project) and on a `marker-changed` event, from its
 * table of what each turn changes (`useSession.session.test.tsx`). This is
 * the read itself: what it shows, and what it shows when it fails.
 *
 * The state under test is what the hook returns, so each case waits for
 * it, or settles a held reply inside `act` (`__tests__/held.ts`), and
 * never for a mock to have been called: that holds on a slow scheduler
 * too (`pnpm test:slow-scheduler`).
 */

import { QueryClientProvider, type QueryClient } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { held } from "../../__tests__/held";
import { createSessionQueryClient } from "../../lib/sessionQueries";
import { READS } from "../../lib/sessionReads";
import type { Marker } from "../../lib/tauri-bridge";

const bridge = vi.hoisted(() => ({
  listMarkers: vi.fn(),
}));

vi.mock("../../lib/tauri-bridge", () => ({
  listMarkers: bridge.listMarkers,
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

let client: QueryClient;

function mount() {
  return renderHook(() => useMarkers(), {
    wrapper: ({ children }: { children: ReactNode }) => (
      <QueryClientProvider client={client}>{children}</QueryClientProvider>
    ),
  });
}

beforeEach(() => {
  client = createSessionQueryClient();
  bridge.listMarkers.mockReset();
});

describe("useMarkers", () => {
  it("reads once at mount and shows the reply", async () => {
    bridge.listMarkers.mockResolvedValue([verse]);
    const { result } = mount();
    expect(result.current).toEqual([]);

    await waitFor(() => expect(result.current).toEqual([verse]));
    expect(bridge.listMarkers).toHaveBeenCalledTimes(1);
  });

  it("shows the new head's labels when the session invalidates the read", async () => {
    // Undo took the label back: the head moved, nothing else was said.
    bridge.listMarkers.mockResolvedValueOnce([verse]).mockResolvedValueOnce([]);
    const { result } = mount();
    await waitFor(() => expect(result.current).toEqual([verse]));

    await act(async () => {
      await client.invalidateQueries({ queryKey: READS.markers });
    });
    await waitFor(() => expect(result.current).toEqual([]));

    // And redo brought it back.
    bridge.listMarkers.mockResolvedValueOnce([verse]);
    await act(async () => {
      await client.invalidateQueries({ queryKey: READS.markers });
    });
    await waitFor(() => expect(result.current).toEqual([verse]));
    expect(bridge.listMarkers).toHaveBeenCalledTimes(3);
  });

  it("keeps the newest reply when an older one arrives last", async () => {
    // A head move while a read is still in flight: the second read is the
    // newer question, and its answer is the one that counts.
    bridge.listMarkers.mockResolvedValueOnce([verse]);
    const { result } = mount();
    await waitFor(() => expect(result.current).toEqual([verse]));
    const older = held<Marker[]>();
    const newer = held<Marker[]>();
    bridge.listMarkers.mockReturnValueOnce(older.promise).mockReturnValueOnce(newer.promise);
    act(() => {
      void client.invalidateQueries({ queryKey: READS.markers });
    });
    act(() => {
      void client.invalidateQueries({ queryKey: READS.markers });
    });

    await newer.resolve([chorus]);
    await waitFor(() => expect(result.current).toEqual([chorus]));

    // The older read was overtaken: its answer is dropped, not drawn.
    await older.resolve([]);
    expect(result.current).toEqual([chorus]);
  });

  it("empties the list when the read is refused", async () => {
    // `NoSession`: a project with nothing in it yet.
    bridge.listMarkers.mockResolvedValueOnce([verse]);
    const { result } = mount();
    await waitFor(() => expect(result.current).toEqual([verse]));

    const refused = held<Marker[]>();
    bridge.listMarkers.mockReturnValueOnce(refused.promise);
    act(() => {
      void client.invalidateQueries({ queryKey: READS.markers });
    });
    await refused.reject("no session loaded; call open_project first");
    await waitFor(() => expect(result.current).toEqual([]));
  });

  it("does not read again until the session says the labels may have changed", async () => {
    bridge.listMarkers.mockResolvedValue([verse]);
    const { result, rerender } = mount();
    await waitFor(() => expect(result.current).toEqual([verse]));
    expect(bridge.listMarkers).toHaveBeenCalledTimes(1);

    rerender();
    expect(bridge.listMarkers).toHaveBeenCalledTimes(1);
  });
});
