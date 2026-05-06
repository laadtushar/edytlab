/**
 * useAgentStream — text deltas accumulate across renders and commit on
 * `agent://done`; tool-call events appear as running badges; node
 * creation resolves the most recent running badge to ✓ and emits a
 * divider.
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
  nodeCreated: [] as ((nodeId: string) => void)[],
  done: [] as (() => void)[],
};

vi.mock("../lib/tauri-bridge", () => ({
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
}));

import { useAgentStream } from "../hooks/useAgentStream";

const flush = () => new Promise<void>((r) => setTimeout(r, 0));

describe("useAgentStream", () => {
  beforeEach(() => {
    cbs.textDelta = [];
    cbs.toolCall = [];
    cbs.nodeCreated = [];
    cbs.done = [];
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

  it("resolves the most recent running badge to ok and emits a node divider", async () => {
    const { result } = renderHook(() => useAgentStream());
    await act(async () => {
      await flush();
    });

    await act(async () => {
      cbs.toolCall[0]("normalize", "tool-1");
      cbs.nodeCreated[0]("a".repeat(64));
    });
    expect(result.current.entries).toHaveLength(2);
    expect(result.current.entries[0]).toMatchObject({
      kind: "tool",
      status: "ok",
    });
    expect(result.current.entries[1]).toMatchObject({
      kind: "node",
      nodeId: "a".repeat(64),
    });
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
});
