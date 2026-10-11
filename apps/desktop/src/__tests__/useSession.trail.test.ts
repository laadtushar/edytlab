/**
 * useSession keeps the path with the project (#453).
 *
 * The path is read back as the project opens and undo uses it, so after a
 * restart undo retraces what the user did. It is saved after every move
 * the user makes, and nothing waits for the save: an edit is a turn in
 * the session's line (`lib/sessionQueue.ts`), and a turn that waited for
 * the disk would hold up every edit behind it.
 *
 * Like `useSession.history.test.ts`, these drive the hook the way the app
 * does and assert through the peeks and `waitFor`, never on a render
 * React may not have drawn yet.
 */

import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { held } from "./held";

const bridge = vi.hoisted(() => ({
  nodeCreated: [] as ((nodeId: string) => void)[],
  getSessionHead: vi.fn(),
  getHeadTrail: vi.fn(),
  saveHeadTrail: vi.fn(),
}));

vi.mock("../lib/tauri-bridge", () => ({
  getSessionHead: bridge.getSessionHead,
  getHeadTrail: bridge.getHeadTrail,
  saveHeadTrail: bridge.saveHeadTrail,
  renderPreview: vi.fn(() => Promise.resolve("/tmp/mix.wav")),
  onNodeCreated: vi.fn((cb: (n: string) => void) => {
    bridge.nodeCreated.push(cb);
    return Promise.resolve(() => {
      bridge.nodeCreated = bridge.nodeCreated.filter((c) => c !== cb);
    });
  }),
}));

import type { SavedTrail } from "../lib/headTrail";
import { useSession } from "../hooks/useSession";

/** Mute, then unmute: the project ends the session on A, having been on M. */
const mutedAndUnmuted: SavedTrail = { head: "A", trail: ["A", "M"], redo: [] };

const nothing: SavedTrail = { head: null, trail: [], redo: [] };

/** Mount, and wait until the head and the saved path have both been read. */
async function mountOnA() {
  const hook = renderHook(() => useSession());
  await waitFor(() => expect(hook.result.current.peekUndo()).not.toBeNull());
  await act(async () => {
    await Promise.allSettled(bridge.getHeadTrail.mock.results.map((r) => r.value));
  });
  return hook;
}

beforeEach(() => {
  bridge.nodeCreated = [];
  bridge.getSessionHead.mockReset().mockResolvedValue("A");
  bridge.getHeadTrail.mockReset().mockResolvedValue(nothing);
  bridge.saveHeadTrail.mockReset().mockResolvedValue(undefined);
});

describe("useSession: the path comes back after a restart", () => {
  it("undo retraces the saved path, not the node's first parent", async () => {
    bridge.getHeadTrail.mockResolvedValue(mutedAndUnmuted);
    const { result } = await mountOnA();

    // The mute, which the node's stored parent does not know.
    expect(result.current.peekUndo()).toEqual({ from: "A", to: "M" });
    act(() => result.current.stepBack("A", "M"));
    expect(result.current.peekUndo()).toEqual({ from: "M", to: "A" });
    act(() => result.current.stepBack("M", "A"));
    // The saved path is spent: now the stored parent is the right answer.
    expect(result.current.peekUndo()).toEqual({ from: "A", to: null });
  });

  it("redo is restored too", async () => {
    bridge.getHeadTrail.mockResolvedValue({ head: "A", trail: [], redo: ["A", "B"] });
    const { result } = await mountOnA();
    expect(result.current.peekRedo()).toEqual({ from: "A", to: "B" });
  });

  it("the restored path shows in `trail`, for the graph to draw", async () => {
    bridge.getHeadTrail.mockResolvedValue(mutedAndUnmuted);
    const { result } = await mountOnA();
    await waitFor(() => expect(result.current.trail).toEqual(["A", "M"]));
  });

  it("a path that ends at another head is not used", async () => {
    // A save lost to a crash, or a store another client has moved on.
    bridge.getHeadTrail.mockResolvedValue({ head: "Z", trail: ["Y"], redo: [] });
    const { result } = await mountOnA();
    expect(result.current.peekUndo()).toEqual({ from: "A", to: null });
    expect(result.current.peekRedo()).toBeNull();
  });

  it.each([
    ["has nothing saved", () => bridge.getHeadTrail.mockResolvedValue(nothing)],
    ["answers with something else", () => bridge.getHeadTrail.mockResolvedValue("garbage")],
    ["answers with nothing", () => bridge.getHeadTrail.mockResolvedValue(undefined)],
    ["refuses", () => bridge.getHeadTrail.mockRejectedValue("no session loaded")],
    [
      "throws",
      () =>
        bridge.getHeadTrail.mockImplementation(() => {
          throw new Error("no such command");
        }),
    ],
  ])("a backend that %s leaves undo on the node's parent, and the project open", async (_, set) => {
    set();
    const { result } = await mountOnA();
    expect(result.current.peekUndo()).toEqual({ from: "A", to: null });
  });

  it("an edit that lands before the saved path is read wins over it", async () => {
    const path = held<SavedTrail>();
    bridge.getHeadTrail.mockReturnValue(path.promise);
    const { result } = renderHook(() => useSession());
    await waitFor(() => expect(result.current.peekUndo()).not.toBeNull());

    act(() => result.current.setHeadLocal("B"));
    await path.resolve(mutedAndUnmuted);

    // Not the saved path (which ends at A): B's own, one step long.
    expect(result.current.peekUndo()).toEqual({ from: "B", to: "A" });
  });

  it("reading the path is not a move: nothing is saved for it", async () => {
    bridge.getHeadTrail.mockResolvedValue(mutedAndUnmuted);
    await mountOnA();
    await act(async () => {
      await Promise.resolve();
    });
    expect(bridge.saveHeadTrail).not.toHaveBeenCalled();
  });
});

describe("useSession: opening a project", () => {
  it("resetHead takes the path saved with that project", async () => {
    const { result } = await mountOnA();
    act(() => result.current.resetHead("X", { head: "X", trail: ["W", "V"], redo: [] }));
    expect(result.current.peekUndo()).toEqual({ from: "X", to: "V" });
  });

  it("resetHead ignores a saved path that ends at another head", async () => {
    const { result } = await mountOnA();
    act(() => result.current.setHeadLocal("B"));
    act(() => result.current.resetHead("X", { head: "Y", trail: ["W"], redo: [] }));
    expect(result.current.peekUndo()).toEqual({ from: "X", to: null });
  });

  it("opening a project is not a move: nothing is saved for it", async () => {
    const { result } = await mountOnA();
    act(() => result.current.resetHead("X", { head: "X", trail: ["W"], redo: [] }));
    await act(async () => {
      await Promise.resolve();
    });
    expect(bridge.saveHeadTrail).not.toHaveBeenCalled();
  });

  it("a save still waiting for the project that was left is dropped", async () => {
    const first = held<void>();
    bridge.saveHeadTrail.mockReturnValueOnce(first.promise);
    const { result } = await mountOnA();

    act(() => result.current.setHeadLocal("B")); // in flight
    await waitFor(() => expect(bridge.saveHeadTrail).toHaveBeenCalledTimes(1));
    act(() => result.current.setHeadLocal("C")); // waiting behind it
    act(() => result.current.resetHead("X"));
    await first.resolve(undefined);
    await act(async () => {
      await Promise.resolve();
    });

    // C's path belongs to the project that was left: never sent.
    expect(bridge.saveHeadTrail).toHaveBeenCalledTimes(1);
  });
});

describe("useSession: the path is saved as the user makes it", () => {
  it("an edit saves the whole path, ending at the new head", async () => {
    const { result } = await mountOnA();
    act(() => result.current.setHeadLocal("M"));
    await waitFor(() =>
      expect(bridge.saveHeadTrail).toHaveBeenLastCalledWith({
        head: "M",
        trail: ["A"],
        redo: [],
      }),
    );

    act(() => result.current.setHeadLocal("A")); // unmute: the node it began on
    await waitFor(() =>
      expect(bridge.saveHeadTrail).toHaveBeenLastCalledWith({
        head: "A",
        trail: ["A", "M"],
        redo: [],
      }),
    );
  });

  it("an agent step saves it too", async () => {
    await mountOnA();
    await waitFor(() => expect(bridge.nodeCreated).toHaveLength(1));
    act(() => bridge.nodeCreated[0]("N"));
    await waitFor(() =>
      expect(bridge.saveHeadTrail).toHaveBeenLastCalledWith({ head: "N", trail: ["A"], redo: [] }),
    );
  });

  it("undo and redo save the trail and the redo list as they stand", async () => {
    const { result } = await mountOnA();
    act(() => result.current.setHeadLocal("M"));
    await waitFor(() => expect(bridge.saveHeadTrail).toHaveBeenCalledTimes(1));

    act(() => result.current.stepBack("M", "A"));
    await waitFor(() =>
      expect(bridge.saveHeadTrail).toHaveBeenLastCalledWith({ head: "A", trail: [], redo: ["M"] }),
    );

    act(() => result.current.stepForward("A", "M"));
    await waitFor(() =>
      expect(bridge.saveHeadTrail).toHaveBeenLastCalledWith({ head: "M", trail: ["A"], redo: [] }),
    );
  });

  it("a head it is already on is not a step, so nothing is saved", async () => {
    const { result } = await mountOnA();
    act(() => result.current.setHeadLocal("A"));
    await act(async () => {
      await Promise.resolve();
    });
    expect(bridge.saveHeadTrail).not.toHaveBeenCalled();
  });

  it("saves what was restored and then moved, so a second restart keeps the whole path", async () => {
    bridge.getHeadTrail.mockResolvedValue(mutedAndUnmuted);
    const { result } = await mountOnA();

    act(() => result.current.stepBack("A", "M"));
    await waitFor(() =>
      expect(bridge.saveHeadTrail).toHaveBeenLastCalledWith({
        head: "M",
        trail: ["A"],
        redo: ["A"],
      }),
    );
  });

  it("a save that fails does not stop the next from being made", async () => {
    bridge.saveHeadTrail.mockRejectedValueOnce("disk full");
    const { result } = await mountOnA();

    act(() => result.current.setHeadLocal("B"));
    await waitFor(() => expect(bridge.saveHeadTrail).toHaveBeenCalledTimes(1));
    act(() => result.current.setHeadLocal("C"));
    await waitFor(() =>
      expect(bridge.saveHeadTrail).toHaveBeenLastCalledWith({
        head: "C",
        trail: ["A", "B"],
        redo: [],
      }),
    );
    expect(result.current.peekUndo()).toEqual({ from: "C", to: "B" });
  });
});

describe("useSession: nothing waits for the save", () => {
  it("an edit in the line finishes while a save is still on its way", async () => {
    const stuck = held<void>();
    bridge.saveHeadTrail.mockReturnValue(stuck.promise);
    const { result } = await mountOnA();

    // The first edit's save never comes back during this test.
    act(() => result.current.setHeadLocal("B"));
    await waitFor(() => expect(bridge.saveHeadTrail).toHaveBeenCalledTimes(1));

    // The next turn in the line starts, adopts its head and ends. A line
    // that waited for the save would leave this pending for good.
    let done = false;
    await act(async () => {
      await result.current.inOrder(async () => {
        result.current.setHeadLocal("C");
      });
      done = true;
    });

    expect(done).toBe(true);
    expect(result.current.peekUndo()).toEqual({ from: "C", to: "B" });
    await stuck.resolve(undefined);
  });

  it("undo and redo are not delayed by a save either", async () => {
    const stuck = held<void>();
    bridge.saveHeadTrail.mockReturnValue(stuck.promise);
    const { result } = await mountOnA();
    act(() => result.current.setHeadLocal("B"));
    await waitFor(() => expect(bridge.saveHeadTrail).toHaveBeenCalledTimes(1));

    const moved = await act(async () =>
      result.current.inOrder(async () => {
        const step = result.current.peekUndo();
        if (!step?.to) return false;
        result.current.stepBack(step.from, step.to);
        return true;
      }),
    );

    expect(moved).toBe(true);
    expect(result.current.peekRedo()).toEqual({ from: "A", to: "B" });
    await stuck.resolve(undefined);
  });

  it("one save at a time, and the newest path is what is written last", async () => {
    const first = held<void>();
    bridge.saveHeadTrail.mockReturnValueOnce(first.promise);
    const { result } = await mountOnA();

    act(() => result.current.setHeadLocal("B"));
    await waitFor(() => expect(bridge.saveHeadTrail).toHaveBeenCalledTimes(1));
    act(() => result.current.setHeadLocal("C"));
    act(() => result.current.setHeadLocal("D"));
    await act(async () => {
      await Promise.resolve();
    });
    // C and D wait for the save in flight.
    expect(bridge.saveHeadTrail).toHaveBeenCalledTimes(1);

    await first.resolve(undefined);
    await waitFor(() => expect(bridge.saveHeadTrail).toHaveBeenCalledTimes(2));
    expect(bridge.saveHeadTrail).toHaveBeenLastCalledWith({
      head: "D",
      trail: ["A", "B", "C"],
      redo: [],
    });
  });
});
