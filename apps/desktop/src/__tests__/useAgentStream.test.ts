/**
 * useAgentStream — text deltas accumulate across renders and commit on
 * `agent://done`; tool-call events appear as running badges; the
 * matching tool-call-end event resolves the badge by id; node-created
 * emits a divider. `awaiting` flips true on `pushUserMessage` and is
 * cleared by any subsequent agent event.
 *
 * The bridge module is mocked so we drive the event stream directly
 * via captured callbacks instead of going through Tauri.
 */

import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

// Mocked event callback registries. Populated as the hook subscribes.
const cbs = {
  textDelta: [] as ((text: string) => void)[],
  toolCall: [] as ((name: string, id: string) => void)[],
  toolCallEnd: [] as ((id: string, ok: boolean) => void)[],
  nodeCreated: [] as ((nodeId: string) => void)[],
  done: [] as (() => void)[],
  plan: [] as ((steps: Record<string, unknown>[]) => void)[],
  planRejected: [] as (() => void)[],
};

const approvePlanMock = vi.hoisted(() => vi.fn());

vi.mock("../lib/tauri-bridge", () => ({
  approvePlan: approvePlanMock,
  rejectPlan: vi.fn(() => Promise.resolve()),
  onTextDelta: vi.fn((cb: (t: string) => void) => {
    cbs.textDelta.push(cb);
    return Promise.resolve(() => {
      cbs.textDelta = cbs.textDelta.filter((c) => c !== cb);
    });
  }),
  onToolCall: vi.fn((cb: (n: string, i: string) => void) => {
    cbs.toolCall.push(cb);
    return Promise.resolve(() => {
      cbs.toolCall = cbs.toolCall.filter((c) => c !== cb);
    });
  }),
  onToolCallEnd: vi.fn((cb: (id: string, ok: boolean) => void) => {
    cbs.toolCallEnd.push(cb);
    return Promise.resolve(() => {
      cbs.toolCallEnd = cbs.toolCallEnd.filter((c) => c !== cb);
    });
  }),
  onNodeCreated: vi.fn((cb: (n: string) => void) => {
    cbs.nodeCreated.push(cb);
    return Promise.resolve(() => {
      cbs.nodeCreated = cbs.nodeCreated.filter((c) => c !== cb);
    });
  }),
  onAgentDone: vi.fn((cb: () => void) => {
    cbs.done.push(cb);
    return Promise.resolve(() => {
      cbs.done = cbs.done.filter((c) => c !== cb);
    });
  }),
  onPlan: vi.fn((cb: (steps: Record<string, unknown>[]) => void) => {
    cbs.plan.push(cb);
    return Promise.resolve(() => {
      cbs.plan = cbs.plan.filter((c) => c !== cb);
    });
  }),
  onPlanUnavailable: vi.fn(() => Promise.resolve(() => undefined)),
  onPlanRejected: vi.fn((cb: () => void) => {
    cbs.planRejected.push(cb);
    return Promise.resolve(() => {
      cbs.planRejected = cbs.planRejected.filter((c) => c !== cb);
    });
  }),
}));

import { useAgentStream } from "../hooks/useAgentStream";
import { held } from "./held";

const flush = () => new Promise<void>((r) => setTimeout(r, 0));

describe("useAgentStream", () => {
  beforeEach(() => {
    cbs.textDelta = [];
    cbs.toolCall = [];
    cbs.toolCallEnd = [];
    cbs.nodeCreated = [];
    cbs.done = [];
    cbs.plan = [];
    cbs.planRejected = [];
    approvePlanMock.mockReset().mockResolvedValue(undefined);
  });

  it("accumulates text deltas into `current` and commits on done", async () => {
    const { result } = renderHook(() => useAgentStream());
    await act(async () => {
      await flush();
    });

    expect(cbs.textDelta).toHaveLength(1);

    await act(async () => {
      cbs.textDelta[0]("Hello, ");
      cbs.textDelta[0]("world!");
    });
    expect(result.current.current).toBe("Hello, world!");
    expect(result.current.entries).toHaveLength(0);

    await act(async () => {
      cbs.done[0]();
    });
    expect(result.current.current).toBe("");
    expect(result.current.entries).toHaveLength(1);
    expect(result.current.entries[0]).toMatchObject({
      kind: "message",
      role: "assistant",
      text: "Hello, world!",
    });
  });

  it("appends a running tool badge on tool-call", async () => {
    const { result } = renderHook(() => useAgentStream());
    await act(async () => {
      await flush();
    });

    await act(async () => {
      cbs.toolCall[0]("normalize", "tool-1");
    });
    expect(result.current.entries).toHaveLength(1);
    expect(result.current.entries[0]).toMatchObject({
      kind: "tool",
      name: "normalize",
      id: "tool-1",
      status: "running",
    });
  });

  it("resolves the running badge by id when tool-call-end arrives and emits a node divider", async () => {
    const { result } = renderHook(() => useAgentStream());
    await act(async () => {
      await flush();
    });

    await act(async () => {
      cbs.toolCall[0]("normalize", "tool-1");
      cbs.toolCallEnd[0]("tool-1", true);
      cbs.nodeCreated[0]("a".repeat(64));
    });
    expect(result.current.entries).toHaveLength(2);
    expect(result.current.entries[0]).toMatchObject({
      kind: "tool",
      id: "tool-1",
      status: "ok",
    });
    expect(result.current.entries[1]).toMatchObject({
      kind: "node",
      nodeId: "a".repeat(64),
    });
  });

  it("marks the badge as error when tool-call-end reports ok=false", async () => {
    const { result } = renderHook(() => useAgentStream());
    await act(async () => {
      await flush();
    });

    await act(async () => {
      cbs.toolCall[0]("gain", "tool-7");
      cbs.toolCallEnd[0]("tool-7", false);
    });
    expect(result.current.entries[0]).toMatchObject({
      kind: "tool",
      id: "tool-7",
      status: "error",
    });
  });

  it("awaiting flips true on submit and clears on first agent event", async () => {
    const { result } = renderHook(() => useAgentStream());
    await act(async () => {
      await flush();
    });

    expect(result.current.awaiting).toBe(false);
    act(() => {
      result.current.pushUserMessage("normalize");
    });
    expect(result.current.awaiting).toBe(true);

    await act(async () => {
      cbs.textDelta[0]("ok");
    });
    expect(result.current.awaiting).toBe(false);
  });

  it("does not commit an empty assistant turn on done", async () => {
    const { result } = renderHook(() => useAgentStream());
    await act(async () => {
      await flush();
    });
    await act(async () => {
      cbs.done[0]();
    });
    expect(result.current.entries).toHaveLength(0);
  });

  it("pushUserMessage appends a user entry", async () => {
    const { result } = renderHook(() => useAgentStream());
    await act(async () => {
      await flush();
    });

    act(() => {
      result.current.pushUserMessage("normalize to -1 dBFS");
    });
    expect(result.current.entries).toHaveLength(1);
    expect(result.current.entries[0]).toMatchObject({
      kind: "message",
      role: "user",
      text: "normalize to -1 dBFS",
    });
  });

  // ------------------------------------------------------------------
  // A held first edit (#415)
  // ------------------------------------------------------------------

  /**
   * With Plan first on and no plan from the model, the text and the tool
   * badge are already on screen when the card for the held edit appears.
   * Declining ends the turn with no `done`, so something else has to
   * settle them: a bubble left "pending" forever, above a card that is
   * gone, is what the user would see.
   */
  it("plan-rejected commits the streamed text and takes the card down", async () => {
    const { result } = renderHook(() => useAgentStream());
    await act(async () => {
      await flush();
    });
    act(() => {
      result.current.pushUserMessage("reverse track 0");
    });

    await act(async () => {
      cbs.textDelta[0]("Reversing track 0.");
      cbs.plan[0]([{ step: 1, tool: "reverse", description: "track: 0" }]);
    });
    expect(result.current.pendingPlan).not.toBeNull();
    expect(result.current.current).toBe("Reversing track 0.");

    await act(async () => {
      cbs.planRejected[0]();
    });
    expect(result.current.pendingPlan).toBeNull();
    expect(result.current.current).toBe("");
    expect(result.current.awaiting).toBe(false);
    expect(result.current.entries).toContainEqual(
      expect.objectContaining({
        kind: "message",
        role: "assistant",
        text: "Reversing track 0.",
      }),
    );
  });

  it("a declined held call's badge resolves", async () => {
    const { result } = renderHook(() => useAgentStream());
    await act(async () => {
      await flush();
    });

    await act(async () => {
      cbs.toolCall[0]("reverse", "t1");
      cbs.plan[0]([{ step: 1, tool: "reverse", description: "track: 0" }]);
    });
    expect(result.current.entries[0]).toMatchObject({ id: "t1", status: "running" });

    await act(async () => {
      cbs.toolCallEnd[0]("t1", false);
      cbs.planRejected[0]();
    });
    expect(result.current.entries[0]).toMatchObject({ id: "t1", status: "error" });
  });

  /**
   * Revising a held edit sends the model off to propose again, and its
   * new proposal is a new card. If that arrives while the answer to the
   * old card is still in flight, resolving the old answer must not take
   * the new card down: nobody could answer it, and the turn would sit
   * parked on a gate nobody can see.
   */
  it("an approval in flight does not take down a newer plan", async () => {
    const answer = held<void>();
    approvePlanMock.mockReturnValue(answer.promise);

    const { result } = renderHook(() => useAgentStream());
    await act(async () => {
      await flush();
    });

    await act(async () => {
      cbs.plan[0]([{ step: 1, tool: "reverse", description: "track: 0" }]);
    });
    const first = result.current.pendingPlan;
    expect(first).not.toBeNull();

    let answered!: Promise<void>;
    act(() => {
      answered = result.current.approvePlan();
    });
    expect(approvePlanMock).toHaveBeenCalledTimes(1);

    await act(async () => {
      cbs.plan[0]([{ step: 1, tool: "reverse", description: "track: 1" }]);
    });
    const second = result.current.pendingPlan;
    expect(second?.steps[0].description).toBe("track: 1");

    await answer.resolve();
    await answered;
    expect(result.current.pendingPlan).toBe(second);
  });

  it("an approval takes down the card it answered", async () => {
    const { result } = renderHook(() => useAgentStream());
    await act(async () => {
      await flush();
    });
    await act(async () => {
      cbs.plan[0]([{ step: 1, tool: "reverse", description: "track: 0" }]);
    });
    expect(result.current.pendingPlan).not.toBeNull();

    await act(async () => {
      await result.current.approvePlan();
    });
    expect(result.current.pendingPlan).toBeNull();
  });
});
