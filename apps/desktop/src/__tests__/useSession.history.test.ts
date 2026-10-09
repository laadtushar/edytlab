/**
 * useSession keeps the path the user took beside the head (#398).
 *
 * Undo and redo ask `peekUndo` / `peekRedo`, which read a ref the reducer
 * writes synchronously. These tests drive the hook the way the app does
 * and assert through the peeks and `act`, never on a render React may not
 * have drawn yet: they must hold when the scheduler is late.
 */

import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const bridge = vi.hoisted(() => ({
  nodeCreated: [] as ((nodeId: string) => void)[],
  getSessionHead: vi.fn(),
  openProject: vi.fn(),
}));

vi.mock("../lib/tauri-bridge", () => ({
  getSessionHead: bridge.getSessionHead,
  openProject: bridge.openProject,
  renderPreview: vi.fn(() => Promise.resolve("/tmp/mix.wav")),
  onNodeCreated: vi.fn((cb: (n: string) => void) => {
    bridge.nodeCreated.push(cb);
    return Promise.resolve(() => {
      bridge.nodeCreated = bridge.nodeCreated.filter((c) => c !== cb);
    });
  }),
}));

import { useSession } from "../hooks/useSession";

/** Mount the hook on a project whose head is "A", once that head has arrived. */
async function mountOnA() {
  const hook = renderHook(() => useSession());
  await waitFor(() => expect(hook.result.current.peekUndo()).not.toBeNull());
  return hook;
}

beforeEach(() => {
  bridge.nodeCreated = [];
  bridge.getSessionHead.mockReset().mockResolvedValue("A");
  bridge.openProject.mockReset();
});

describe("useSession: the path taken", () => {
  it("sees a move the moment it is made, before React renders it", async () => {
    const { result } = await mountOnA();
    expect(result.current.peekUndo()).toEqual({ from: "A", to: null });

    // One `act`: nothing has rendered between the move and the peek. A
    // handler that closed over the last render would still say "A".
    act(() => {
      result.current.setHeadLocal("B");
      expect(result.current.peekUndo()).toEqual({ from: "B", to: "A" });
    });
  });

  it("answers from the latest state through a function taken from an old render", async () => {
    const { result } = await mountOnA();
    const { peekUndo, setHeadLocal } = result.current;

    act(() => setHeadLocal("B"));
    act(() => setHeadLocal("C"));

    expect(peekUndo()).toEqual({ from: "C", to: "B" });
  });

  it("has nothing to undo or redo with no head", async () => {
    bridge.getSessionHead.mockRejectedValue("no session loaded");
    const { result } = renderHook(() => useSession());
    // Let the refused read land, then ask.
    await act(async () => {
      await Promise.allSettled(bridge.getSessionHead.mock.results.map((r) => r.value));
    });
    expect(result.current.peekUndo()).toBeNull();
    expect(result.current.peekRedo()).toBeNull();
  });

  it("mute, unmute: undo retraces the steps, then asks the node for its parent", async () => {
    const { result } = await mountOnA();
    act(() => result.current.setHeadLocal("M")); // mute
    act(() => result.current.setHeadLocal("A")); // unmute: the node it started on

    expect(result.current.peekUndo()).toEqual({ from: "A", to: "M" });
    act(() => result.current.stepBack("A", "M"));
    expect(result.current.peekUndo()).toEqual({ from: "M", to: "A" });
    act(() => result.current.stepBack("M", "A"));
    // The trail is spent: `to` is null and the app reads the parent.
    expect(result.current.peekUndo()).toEqual({ from: "A", to: null });
  });

  it("undo arms redo, and redo walks back the way undo came", async () => {
    const { result } = await mountOnA();
    act(() => result.current.setHeadLocal("B"));
    expect(result.current.peekRedo()).toBeNull();

    act(() => result.current.stepBack("B", "A"));
    expect(result.current.peekRedo()).toEqual({ from: "A", to: "B" });

    act(() => result.current.stepForward("A", "B"));
    expect(result.current.peekRedo()).toBeNull();
    expect(result.current.peekUndo()).toEqual({ from: "B", to: "A" });
  });

  it("an edit after an undo leaves nothing to redo", async () => {
    const { result } = await mountOnA();
    act(() => result.current.setHeadLocal("B"));
    act(() => result.current.stepBack("B", "A"));
    expect(result.current.peekRedo()).not.toBeNull();

    act(() => result.current.setHeadLocal("C")); // a UI edit
    expect(result.current.peekRedo()).toBeNull();
    expect(result.current.peekUndo()).toEqual({ from: "C", to: "A" });
  });

  it("an agent node after an undo leaves nothing to redo either", async () => {
    const { result } = await mountOnA();
    await waitFor(() => expect(bridge.nodeCreated).toHaveLength(1));
    act(() => result.current.setHeadLocal("B"));
    act(() => result.current.stepBack("B", "A"));
    expect(result.current.peekRedo()).toEqual({ from: "A", to: "B" });

    act(() => bridge.nodeCreated[0]("C"));
    expect(result.current.peekRedo()).toBeNull();
    expect(result.current.peekUndo()).toEqual({ from: "C", to: "A" });
  });

  it("resetHead forgets the old project's path and redo list", async () => {
    const { result } = await mountOnA();
    act(() => result.current.setHeadLocal("B"));
    act(() => result.current.setHeadLocal("C"));
    act(() => result.current.stepBack("C", "B"));
    expect(result.current.peekRedo()).not.toBeNull();

    act(() => result.current.resetHead("X"));
    expect(result.current.peekUndo()).toEqual({ from: "X", to: null });
    expect(result.current.peekRedo()).toBeNull();

    // A project with nothing in it has no head to undo from.
    act(() => result.current.resetHead(null));
    expect(result.current.peekUndo()).toBeNull();
  });

  it("opening a project through the hook resets the path too", async () => {
    const { result } = await mountOnA();
    act(() => result.current.setHeadLocal("B"));
    bridge.openProject.mockResolvedValue({ path: "/p/other", head: "X" });

    await act(async () => {
      await result.current.openProject("/p/other");
    });

    expect(result.current.peekUndo()).toEqual({ from: "X", to: null });
  });

  it("exposes the trail and head for the graph to draw", async () => {
    const { result } = await mountOnA();
    act(() => result.current.setHeadLocal("B"));
    act(() => result.current.setHeadLocal("A"));

    await waitFor(() => expect(result.current.head).toBe("A"));
    expect(result.current.trail).toEqual(["A", "B"]);
  });
});
