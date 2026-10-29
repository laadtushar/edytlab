import { onlineManager, QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import React, { useEffect, useReducer, useState } from "react";
import { beforeEach, expect, it, vi } from "vitest";
import { held } from "./held";

const bridge = vi.hoisted(() => ({
  nodeCreated: [] as ((nodeId: string) => void)[],
  listTracks: vi.fn(),
}));

vi.mock("../lib/tauri-bridge", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../lib/tauri-bridge")>();
  return {
    isNoSession: actual.isNoSession,
    listTracks: bridge.listTracks,
    getSessionHead: vi.fn(() => Promise.resolve("A")),
    getHeadTrail: vi.fn(() => Promise.resolve({ head: null, trail: [], redo: [] })),
    saveHeadTrail: vi.fn(() => Promise.resolve()),
    renderPreview: vi.fn(() => Promise.resolve("/tmp/mix.wav")),
    onNodeCreated: vi.fn((cb: (n: string) => void) => {
      bridge.nodeCreated.push(cb);
      return Promise.resolve(() => {
        bridge.nodeCreated = bridge.nodeCreated.filter((c) => c !== cb);
      });
    }),
    onToolCall: vi.fn(() => Promise.resolve(() => undefined)),
    onToolCallEnd: vi.fn(() => Promise.resolve(() => undefined)),
    onAgentDone: vi.fn(() => Promise.resolve(() => undefined)),
    getGraph: vi.fn(() => Promise.resolve({ nodes: [], head: null })),
  };
});

import { useSession } from "../hooks/useSession";
import { createSessionQueryClient } from "../lib/sessionQueries";
import { OwnedControls, type Claim } from "../lib/ownedControls";
import type { TrackSummary } from "../lib/tauri-bridge";

const tracks = (gain: number, name = "Voice"): TrackSummary[] => [{
  id: "voice", name, muted: false, gain_db: gain, pan: 0, soloed: false,
  audio_path: "/tmp/voice.wav", clips: [],
}];

function fixture({ client = createSessionQueryClient(), strict = false } = {}) {
  function Wrapper({ children }: { children: React.ReactNode }) {
    return <QueryClientProvider client={client}>{children}</QueryClientProvider>;
  }
  return { client, wrapper: Wrapper, reactStrictMode: strict };
}

async function mountTracks() {
  const query = fixture();
  const hook = renderHook(() => {
    const [, changed] = useReducer((n: number) => n + 1, 0);
    const [owner] = useState(() => new OwnedControls(changed));
    const session = useSession({ trackControls: owner });
    return { ...session, owner, visible: owner.overlay(session.tracks) };
  }, query);
  await waitFor(() => expect(hook.result.current.head).toBe("A"));
  expect(hook.result.current.tracks).toEqual([]);
  return { ...hook, client: query.client };
}

async function readBaseline(hook: Awaited<ReturnType<typeof mountTracks>>, gain = 0) {
  bridge.listTracks.mockResolvedValueOnce(tracks(gain));
  await act(() => hook.result.current.refreshTracks());
  await waitFor(() => expect(hook.result.current.tracks).toEqual(tracks(gain)));
}

function sendGain(owner: OwnedControls, gain: number) {
  owner.set("voice", "gain", gain);
  return owner.send("voice", "gain", gain);
}

// Compile-only contract. Never invoke hooks outside a component.
function claimRequiresOwner(claim: Claim, owner: OwnedControls) {
  // @ts-expect-error A claim without its owning controls cannot be confirmed.
  useSession().refreshTracks(claim);
  void useSession({ trackControls: owner }).refreshTracks(claim);
}
void claimRequiresOwner;

beforeEach(() => {
  bridge.nodeCreated = [];
  bridge.listTracks.mockReset().mockResolvedValue([]);
});

it("exposes an empty list without starting a track read at mount", async () => {
  const client = new QueryClient();
  function Wrapper({ children }: { children: React.ReactNode }) {
    return <QueryClientProvider client={client}>{children}</QueryClientProvider>;
  }
  const hook = renderHook(() => useSession(), { wrapper: Wrapper });
  await waitFor(() => expect(hook.result.current.head).toBe("A"));
  expect(hook.result.current).toMatchObject({ tracks: [] });
  expect(bridge.listTracks).not.toHaveBeenCalled();
});

it("supersedes an initial no-data read without confirming the newer claim early", async () => {
  const hook = await mountTracks();
  const initial = held<TrackSummary[]>();
  const edited = held<TrackSummary[]>();
  bridge.listTracks.mockReturnValueOnce(initial.promise).mockReturnValueOnce(edited.promise);
  let old!: Promise<void>;
  act(() => { old = hook.result.current.refreshTracks(); });
  await waitFor(() => expect(bridge.listTracks).toHaveBeenCalledTimes(1));
  const claim = sendGain(hook.result.current.owner, -3);
  let latest!: Promise<void>;
  act(() => { latest = hook.result.current.refreshTracks(claim); });
  await waitFor(() => expect(bridge.listTracks).toHaveBeenCalledTimes(2));
  expect(hook.result.current.owner.overlay(tracks(0))[0].gain_db).toBe(-3);
  await edited.resolve(tracks(-3));
  await act(() => latest);
  await waitFor(() => expect(hook.result.current.tracks).toEqual(tracks(-3)));
  await waitFor(() => expect(hook.result.current.owner.sent("voice", "gain")).toBeUndefined());
  await initial.resolve(tracks(0));
  await act(() => old);
  await waitFor(() => expect(hook.result.current.tracks).toEqual(tracks(-3)));
});

it("starts independent overlapping refreshes instead of reusing the initial promise", async () => {
  const hook = await mountTracks();
  const first = held<TrackSummary[]>();
  const second = held<TrackSummary[]>();
  bridge.listTracks.mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise);
  let older!: Promise<void>;
  act(() => { older = hook.result.current.refreshTracks(); });
  await waitFor(() => expect(bridge.listTracks).toHaveBeenCalledTimes(1));
  let newer!: Promise<void>;
  act(() => { newer = hook.result.current.refreshTracks(); });
  await waitFor(() => expect(bridge.listTracks).toHaveBeenCalledTimes(2));
  await second.resolve(tracks(-3));
  await act(() => newer);
  await waitFor(() => expect(hook.result.current.tracks).toEqual(tracks(-3)));
  await first.resolve(tracks(0));
  await act(() => older);
  expect(hook.result.current.tracks).toEqual(tracks(-3));
});

it("retains the last valid list and held control when the confirming read fails", async () => {
  const hook = await mountTracks();
  await readBaseline(hook);
  const failing = held<TrackSummary[]>();
  bridge.listTracks.mockReturnValueOnce(failing.promise);
  const claim = sendGain(hook.result.current.owner, -3);
  let answer!: Promise<unknown>;
  hook.rerender();
  expect(hook.result.current.visible).toEqual(tracks(-3));
  act(() => {
    answer = hook.result.current.refreshTracks(claim).catch((error: unknown) => error);
  });
  await waitFor(() => expect(bridge.listTracks).toHaveBeenCalledTimes(2));
  const error = new Error("read failed");
  await failing.reject(error);
  expect(await answer).toBe(error);
  await waitFor(() => expect(hook.result.current.visible).toEqual(tracks(-3)));
  expect(hook.result.current.tracks).toEqual(tracks(0));
  expect(hook.result.current.owner.sent("voice", "gain")).toBe(-3);
});

it("settles an obsolete rejection quietly after a later success", async () => {
  const hook = await mountTracks();
  await readBaseline(hook);
  const oldRead = held<TrackSummary[]>();
  bridge.listTracks.mockReturnValueOnce(oldRead.promise).mockResolvedValueOnce(tracks(-3));
  let old!: Promise<unknown>;
  act(() => { old = hook.result.current.refreshTracks().catch((error: unknown) => error); });
  await waitFor(() => expect(bridge.listTracks).toHaveBeenCalledTimes(2));
  await act(() => hook.result.current.refreshTracks());
  await waitFor(() => expect(hook.result.current.tracks).toEqual(tracks(-3)));
  await oldRead.reject(new Error("obsolete failure"));
  expect(await old).toBeUndefined();
  expect(hook.result.current.tracks).toEqual(tracks(-3));
});

it("adopts NoSession as a successful empty list", async () => {
  const hook = await mountTracks();
  await readBaseline(hook);
  bridge.listTracks.mockRejectedValueOnce("no session loaded; call open_project first");
  await act(() => hook.result.current.refreshTracks());
  await waitFor(() => expect(hook.result.current.tracks).toEqual([]));
});

it("confirms identical-data success even when the track array is structurally shared", async () => {
  const hook = await mountTracks();
  await readBaseline(hook);
  const previous = hook.result.current.tracks;
  const confirming = held<TrackSummary[]>();
  bridge.listTracks.mockReturnValueOnce(confirming.promise);
  const claim = sendGain(hook.result.current.owner, -3);
  let done!: Promise<void>;
  act(() => { done = hook.result.current.refreshTracks(claim); });
  await waitFor(() => expect(bridge.listTracks).toHaveBeenCalledTimes(2));
  expect(hook.result.current.owner.overlay(previous)[0].gain_db).toBe(-3);
  await confirming.resolve(tracks(0));
  await act(() => done);
  await waitFor(() => expect(hook.result.current.visible).toEqual(tracks(0)));
  expect(hook.result.current.tracks).toBe(previous);
  expect(hook.result.current.owner.sent("voice", "gain")).toBeUndefined();
});

it("does not release a newer claim when rerendering an older cached success", async () => {
  const hook = await mountTracks();
  await readBaseline(hook);
  const confirming = held<TrackSummary[]>();
  bridge.listTracks.mockReturnValueOnce(confirming.promise);
  const claim = sendGain(hook.result.current.owner, -3);
  let done!: Promise<void>;
  act(() => { done = hook.result.current.refreshTracks(claim); });
  hook.rerender();
  await waitFor(() => expect(bridge.listTracks).toHaveBeenCalledTimes(2));
  expect(hook.result.current.owner.overlay(tracks(0))[0].gain_db).toBe(-3);
  await confirming.resolve(tracks(-3));
  await act(() => done);
  await waitFor(() => expect(hook.result.current.owner.sent("voice", "gain")).toBeUndefined());
});

it("keeps newer unsent input held when a prior sent value is confirmed", async () => {
  const hook = await mountTracks();
  await readBaseline(hook);
  const confirming = held<TrackSummary[]>();
  bridge.listTracks.mockReturnValueOnce(confirming.promise);
  const claim = sendGain(hook.result.current.owner, -3);
  let done!: Promise<void>;
  act(() => { done = hook.result.current.refreshTracks(claim); });
  act(() => hook.result.current.owner.set("voice", "gain", -6));
  await confirming.resolve(tracks(-3));
  await act(() => done);
  await waitFor(() => expect(hook.result.current.tracks).toEqual(tracks(-3)));
  expect(hook.result.current.visible).toEqual(tracks(-6));
});

it("supersedes a read across same-head same-track reset without releasing new-project input", async () => {
  const hook = await mountTracks();
  await readBaseline(hook);
  const oldRead = held<TrackSummary[]>();
  bridge.listTracks.mockReturnValueOnce(oldRead.promise).mockResolvedValueOnce(tracks(-9, "New project"));
  const oldClaim = sendGain(hook.result.current.owner, -3);
  let old!: Promise<void>;
  act(() => { old = hook.result.current.refreshTracks(oldClaim); });
  await waitFor(() => expect(bridge.listTracks).toHaveBeenCalledTimes(2));
  act(() => {
    hook.result.current.owner.clear();
    hook.result.current.resetHead("A");
    hook.result.current.owner.set("voice", "gain", -6);
  });
  await act(() => hook.result.current.refreshTracks());
  await waitFor(() => expect(hook.result.current.tracks).toEqual(tracks(-9, "New project")));
  await oldRead.resolve(tracks(0));
  await act(() => old);
  expect(hook.result.current.head).toBe("A");
  expect(hook.result.current.tracks).toEqual(tracks(-9, "New project"));
  expect(hook.result.current.visible).toEqual(tracks(-6, "New project"));
});

it("cancels a reset read immediately even before the next project's refresh starts", async () => {
  const hook = await mountTracks();
  await readBaseline(hook);
  const oldRead = held<TrackSummary[]>();
  bridge.listTracks.mockReturnValueOnce(oldRead.promise);
  let old!: Promise<void>;
  act(() => { old = hook.result.current.refreshTracks(); });
  await waitFor(() => expect(bridge.listTracks).toHaveBeenCalledTimes(2));
  act(() => hook.result.current.resetHead("A"));
  await oldRead.resolve(tracks(-9, "Old project"));
  await act(() => old);
  expect(hook.result.current.tracks).toEqual(tracks(0));
});

it("isolates simultaneously mounted session clients", async () => {
  const first = await mountTracks();
  await readBaseline(first, -3);
  const second = await mountTracks();
  await readBaseline(second, -9);
  expect(first.client).not.toBe(second.client);
  expect(first.result.current.tracks).toEqual(tracks(-3));
  expect(second.result.current.tracks).toEqual(tracks(-9));
  act(() => first.result.current.setHeadLocal("B"));
  await waitFor(() => expect(first.result.current.head).toBe("B"));
  expect(bridge.listTracks).toHaveBeenCalledTimes(2);
});

it("cancels a held read on final unmount so it cannot populate the cache or confirm ownership", async () => {
  const hook = await mountTracks();
  const oldRead = held<TrackSummary[]>();
  bridge.listTracks.mockReturnValueOnce(oldRead.promise);
  const owner = hook.result.current.owner;
  const claim = sendGain(owner, -3);
  let old!: Promise<void>;
  act(() => { old = hook.result.current.refreshTracks(claim); });
  await waitFor(() => expect(bridge.listTracks).toHaveBeenCalledTimes(1));
  hook.unmount();
  await oldRead.resolve(tracks(0));
  await act(() => old);
  // Inspect the actual cache without exposing the private query key.
  expect(hook.client.getQueryCache().getAll().every((query) => query.state.data === undefined)).toBe(true);
  expect(owner.overlay(tracks(0))).toEqual(tracks(-3));
  const next = await mountTracks();
  await readBaseline(next, -9);
  expect(next.result.current.tracks).toEqual(tracks(-9));
});

it("invalidates the prior effect lifetime on StrictMode replay and still accepts the replay boot", async () => {
  const replayRead = held<TrackSummary[]>();
  bridge.listTracks.mockReturnValueOnce(replayRead.promise);
  const query = fixture({ strict: true });
  const boots: Promise<unknown>[] = [];
  const hook = renderHook(() => {
    const session = useSession();
    useEffect(() => {
      // Cleanup overtakes the first boot while it awaits cancellation.
      const boot = session.refreshTracks();
      boots.push(boot.catch((error: unknown) => error));
    }, [session.refreshTracks]);
    return session;
  }, query);
  await waitFor(() => expect(hook.result.current.head).toBe("A"));
  expect(boots).toHaveLength(2);
  await waitFor(() => expect(bridge.listTracks).toHaveBeenCalledTimes(1));
  await replayRead.resolve(tracks(-3));
  await waitFor(() => expect(hook.result.current.tracks).toEqual(tracks(-3)));
  await act(async () => { await Promise.all(boots); });
  expect(hook.result.current.tracks).toEqual(tracks(-3));
  await waitFor(() => expect(bridge.nodeCreated).toHaveLength(1));
  hook.unmount();
  await waitFor(() => expect(bridge.nodeCreated).toHaveLength(0));
});

it("keeps explicit local IPC reads working while the query online manager is offline", async () => {
  const wasOnline = onlineManager.isOnline();
  onlineManager.setOnline(false);
  try {
    const hook = await mountTracks();
    await readBaseline(hook, -3);
    expect(hook.result.current.tracks).toEqual(tracks(-3));
  } finally {
    onlineManager.setOnline(wasOnline);
  }
});
