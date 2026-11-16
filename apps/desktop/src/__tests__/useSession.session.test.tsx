/**
 * `useSession` is the only way the window changes the session (ADR 0001).
 *
 * These drive the hook against a fake backend behind the IPC boundary, with
 * the reads a window shows (tracks, graph, labels, transcript) mounted beside
 * it. They pin the rules the two regex scans (`headIsApplied`,
 * `sessionQueueCoverage`) and `lib/ownedControls.ts` used to hold, as
 * behaviour of the module's own `edit` and `move`:
 *
 * - every edit in the table has its head adopted, and goes in the line;
 * - a control the user holds stays theirs until the read after its commit
 *   has landed, and is let go of when the session moves by itself;
 * - each turn re-reads what its table entry says, and nothing else.
 *
 * Nothing here waits on a render React may not have drawn yet: answers are
 * held and delivered inside `act` (`held.ts`), so the tests hold when the
 * scheduler is late (#349).
 */

import { onlineManager, QueryClientProvider, useQuery } from "@tanstack/react-query";
import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { emit } from "@tauri-apps/api/event";
import { clearMocks } from "@tauri-apps/api/mocks";
import { StrictMode, type ReactNode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { useMarkers } from "../hooks/useMarkers";
import { useSession, type UseSessionResult } from "../hooks/useSession";
import { useTranscript } from "../hooks/useTranscript";
import { edits, type EditArgs, type EditName } from "../lib/sessionCommands";
import { SessionQueryProvider, createSessionQueryClient } from "../lib/sessionQueries";
import { READS, readGraph } from "../lib/sessionReads";
import {
  type Args,
  HeldTrackReads,
  appAnswer,
  bootIpc,
  node,
  settle,
  voiceTrack,
} from "./appHarness";
import { held, type Held } from "./held";

const A = node(1);
const B = node(2);
const C = node(3);
const GONE = node(9);

/** The bridge's answer for a store holding `ids`. */
const graphOf = (head: string, ids: string[]) => ({
  nodes: ids.map((id) => ({ id, parent: null, label: null, tool: null, created_at: "" })),
  head,
});

/** The IPC commands of the `edits` table, which the backend answers with a new head. */
const EDIT_COMMANDS = [
  "set_track_gain",
  "set_track_pan",
  "set_track_muted",
  "set_track_soloed",
  "set_sync_lock",
  "rename_track",
  "remove_track",
  "duplicate_track",
  "move_clip",
  "remove_clip",
  "set_clip_envelope",
  "add_marker",
  "remove_marker",
  "update_marker",
  "cut_transcript_words",
  "apply_template",
];

class Backend extends HeldTrackReads {
  calls = 0;
  head = A;
  gain = 0;
  /** The nodes the store holds. */
  nodes = [A, B, C];
  /** Every command sent, in order. */
  readonly sent: { cmd: string; args: Args }[] = [];
  /** Commands whose answer the test holds: the first one of each name. */
  readonly gates = new Map<string, Held<unknown>>();
  /** Commands that are refused, by name. */
  readonly refuse = new Map<string, string>();
  private next = 10;

  count(cmd: string): number {
    return this.sent.filter((s) => s.cmd === cmd).length;
  }

  handle = (cmd: string, args: Args = {}): unknown => {
    this.calls++;
    this.sent.push({ cmd, args });
    switch (cmd) {
      case "list_tracks":
        return this.heldRead([voiceTrack({ gain_db: this.gain })]);
      case "get_session_head":
        return this.head;
      case "get_graph":
        return graphOf(this.head, this.nodes);
      case "get_head_trail":
        return { head: null, trail: [], redo: [] };
      case "save_head_trail":
        return null;
      case "get_node":
        return { id: args.id, parent: args.id === A ? null : A };
      case "set_head_to": {
        const refused = this.refuse.get(cmd);
        if (refused) throw refused;
        const to = args.nodeId as string;
        if (!this.nodes.includes(to)) throw "node not found";
        this.head = to;
        return to;
      }
      case "accept_b":
        this.head = args.b as string;
        return this.head;
      case "open_project":
        return { head: this.head };
      case "save_project_as":
        return { files: 1, bytes: 1, skipped_cache_files: 0 };
      case "batch_load":
        return { tracks_loaded: 1, last_node_id: this.advance(), failures: [] };
      case "rename_node":
        return null;
      default:
        break;
    }
    if (EDIT_COMMANDS.includes(cmd)) {
      const refused = this.refuse.get(cmd);
      const gate = this.gates.get(cmd);
      const answer = () => {
        if (refused) throw refused;
        if (cmd === "set_track_gain") this.gain = args.gainDb as number;
        return this.advance();
      };
      if (gate) {
        this.gates.delete(cmd);
        return gate.promise.then(answer);
      }
      return answer();
    }
    return appAnswer(cmd);
  };

  private advance(): string {
    this.head = node(this.next++);
    this.nodes.push(this.head);
    return this.head;
  }
}

let backend: Backend;
let readError: unknown[];

/** The window's reads beside the session: what shows the session's state. */
function useWindow() {
  const session = useSession({ onReadError: (e) => readError.push(e) });
  const markers = useMarkers();
  const transcript = useTranscript();
  const graph = useQuery({ queryKey: READS.graph, queryFn: readGraph });
  return { session, markers, transcript, graph };
}

async function mount() {
  const hook = renderHook(useWindow, { wrapper: SessionQueryProvider });
  await waitFor(() => expect(hook.result.current.session.head).toBe(A));
  await waitFor(() => expect(hook.result.current.session.sessionTracks).toHaveLength(1));
  await settle();
  await backend.deliverAll();
  return hook;
}

const gainOf = (session: UseSessionResult) => session.tracks[0].gain_db;

beforeEach(() => {
  backend = new Backend();
  readError = [];
  bootIpc(backend);
});

afterEach(() => {
  cleanup();
  clearMocks();
});

/** One sample of every entry of the table, typed so a new entry needs one. */
const SAMPLES: { [K in EditName]: { args: EditArgs<K> } } = {
  setTrackGain: { args: [0, -3] },
  setTrackPan: { args: [0, 0.5] },
  setTrackMuted: { args: [0, true] },
  setTrackSoloed: { args: [0, true] },
  setSyncLock: { args: [true] },
  renameTrack: { args: [0, "Voice"] },
  removeTrack: { args: [0] },
  duplicateTrack: { args: [0] },
  moveClip: { args: [0, 0, 1] },
  removeClip: { args: [0, 0] },
  setClipEnvelope: { args: [0, 0, []] },
  addMarker: { args: [1, "verse"] },
  removeMarker: { args: ["m1"] },
  updateMarker: { args: ["m1", { name: "chorus" }] },
  cutTranscriptWords: { args: [0, 1, 2] },
  applyTemplate: { args: ["podcast"] },
  batchLoad: { args: [["/tmp/a.wav"]] },
  renameNode: { args: [A, "intro"] },
};

describe("every edit in the table", () => {
  const names = Object.keys(edits) as EditName[];

  it("has a sample here", () => {
    expect(Object.keys(SAMPLES).sort()).toEqual([...names].sort());
  });

  it.each(names.filter((name) => name !== "renameNode"))(
    "%s: the head it returns is adopted, and the edit is one turn in the line",
    async (name) => {
      const { result } = await mount();
      const before = result.current.session.head;
      const edition = result.current.session.edition;

      await act(async () => {
        await result.current.session.edit(name, SAMPLES[name].args as never);
      });

      expect(result.current.session.head).not.toBe(before);
      expect(result.current.session.head).toBe(backend.head);
      // The step it leaves is on the path undo takes.
      expect(result.current.session.trail).toContain(before);
      // A mix rendered before it is no mix of the session now.
      expect(result.current.session.edition).toBeGreaterThan(edition);
    },
  );

  it("renameNode names a node and moves no head", async () => {
    const { result } = await mount();
    const edition = result.current.session.edition;
    await act(async () => {
      await result.current.session.edit("renameNode", [A, "intro"]);
    });
    expect(result.current.session.head).toBe(A);
    expect(result.current.session.edition).toBe(edition);
    expect(backend.count("rename_node")).toBe(1);
  });
});

describe("the line", () => {
  it("sends edits one at a time, in the order they were made", async () => {
    const { result } = await mount();
    const first = held<unknown>();
    backend.gates.set("set_track_gain", first);

    let one!: Promise<unknown>;
    let two!: Promise<unknown>;
    act(() => {
      one = result.current.session.edit("setTrackGain", [0, -3]);
      two = result.current.session.edit("setTrackGain", [0, 0]);
    });
    await settle();

    // The second waits for the first to be answered.
    expect(backend.count("set_track_gain")).toBe(1);
    await first.resolve(undefined);
    await act(async () => {
      await Promise.all([one, two]);
    });

    const gains = backend.sent.filter((s) => s.cmd === "set_track_gain").map((s) => s.args.gainDb);
    expect(gains).toEqual([-3, 0]);
    // The head is the second answer's, not the first's.
    expect(result.current.session.head).toBe(backend.head);
  });

  it("goes on after an edit that failed, and the failure reaches whoever made it", async () => {
    const { result } = await mount();
    backend.refuse.set("set_track_gain", "out of range");

    let failure: unknown;
    await act(async () => {
      await result.current.session
        .edit("setTrackGain", [0, 99])
        .catch((error: unknown) => (failure = error));
    });
    backend.refuse.clear();
    await act(async () => {
      await result.current.session.edit("setTrackMuted", [0, true]);
    });

    expect(String(failure)).toContain("out of range");
    expect(backend.count("set_track_muted")).toBe(1);
  });

  it("does not hold the next edit up for the read after the one before it", async () => {
    const { result } = await mount();
    backend.holdReads = true;

    await act(async () => {
      await result.current.session.edit("setTrackGain", [0, -3]);
    });
    // The read after the first edit has not been answered.
    expect(backend.pendingReads).toHaveLength(1);

    await act(async () => {
      await result.current.session.edit("setTrackGain", [0, -6]);
    });
    expect(backend.count("set_track_gain")).toBe(2);
    await backend.deliverAll();
  });

  it("runs undo behind an edit, from the head that edit made", async () => {
    const { result } = await mount();
    const gate = held<unknown>();
    backend.gates.set("set_track_gain", gate);

    let edit!: Promise<unknown>;
    let undo!: Promise<boolean>;
    act(() => {
      edit = result.current.session.edit("setTrackGain", [0, -3]);
      undo = result.current.session.move({ kind: "undo" });
    });
    await settle();
    expect(backend.count("set_head_to")).toBe(0);
    await gate.resolve(undefined);
    await act(async () => {
      await edit;
    });
    await act(async () => {
      expect(await undo).toBe(true);
    });

    // Back to A: the head before the edit, not the one before that.
    expect(backend.sent.filter((s) => s.cmd === "set_head_to").map((s) => s.args.nodeId)).toEqual([A]);
    expect(result.current.session.head).toBe(A);
  });
});

describe("a lane control the user holds", () => {
  it("is the user's from the input event until the read after its commit has landed", async () => {
    const { result } = await mount();

    act(() => result.current.session.holdInput({ index: 0, control: "gain", value: -3 }));
    // A list that arrives meanwhile does not put the older value back.
    await act(async () => {
      await result.current.session.edit("setTrackMuted", [0, false]);
    });
    await backend.deliverAll();
    expect(gainOf(result.current.session)).toBe(-3);

    backend.holdReads = true;
    await act(async () => {
      await result.current.session.edit("setTrackGain", [0, -3], {
        hold: { index: 0, control: "gain", value: -3 },
      });
    });
    expect(backend.pendingReads).toHaveLength(1);
    // Committed and answered, and the session has not been read: still theirs.
    expect(gainOf(result.current.session)).toBe(-3);
    expect(result.current.session.sentValue(0, "gain")).toBe(-3);

    await backend.deliverAll();
    expect(result.current.session.sentValue(0, "gain")).toBeUndefined();
    expect(gainOf(result.current.session)).toBe(-3);
    expect(result.current.session.sessionTracks[0].gain_db).toBe(-3);
  });

  it("is not confirmed by a read asked for before the commit was answered", async () => {
    const { result } = await mount();
    backend.holdReads = true;
    // A read of the older list is on its way.
    act(() => {
      void result.current.graph.refetch();
    });
    const gate = held<unknown>();
    backend.gates.set("set_track_gain", gate);

    let edit!: Promise<unknown>;
    act(() => {
      edit = result.current.session.edit("setTrackGain", [0, -3], {
        hold: { index: 0, control: "gain", value: -3 },
      });
    });
    await settle();
    await gate.resolve(undefined);
    await act(async () => {
      await edit;
    });
    await settle();

    // Only the read asked for after the answer can hand the control back.
    expect(gainOf(result.current.session)).toBe(-3);
    expect(result.current.session.sentValue(0, "gain")).toBe(-3);
    await backend.deliverAll();
    expect(result.current.session.sentValue(0, "gain")).toBeUndefined();
  });

  it("the newest value of a control wins while two commits are in the line", async () => {
    const { result } = await mount();
    backend.holdReads = true;
    const gate = held<unknown>();
    backend.gates.set("set_track_gain", gate);

    act(() => {
      void result.current.session.edit("setTrackGain", [0, -3], {
        hold: { index: 0, control: "gain", value: -3 },
      });
      void result.current.session.edit("setTrackGain", [0, 0], {
        hold: { index: 0, control: "gain", value: 0 },
      });
    });
    await settle();
    expect(result.current.session.sentValue(0, "gain")).toBe(0);
    await gate.resolve(undefined);
    await backend.deliverAll();

    expect(result.current.session.sentValue(0, "gain")).toBeUndefined();
    expect(gainOf(result.current.session)).toBe(0);
  });

  it("goes back to the session's value when the session refused the commit", async () => {
    const { result } = await mount();
    backend.refuse.set("set_track_gain", "out of range");
    const listBefore = result.current.session.sessionTracks;

    await act(async () => {
      await result.current.session
        .edit("setTrackGain", [0, 99], { hold: { index: 0, control: "gain", value: 99 } })
        .catch(() => undefined);
    });
    await backend.deliverAll();

    expect(gainOf(result.current.session)).toBe(0);
    // A new list, for a lane that must draw the session's value again.
    expect(result.current.session.sessionTracks).not.toBe(listBefore);
  });

  it("a slider's input is held beside the list, and dropping it hands it back", async () => {
    const { result } = await mount();

    act(() => result.current.session.holdInput({ index: 0, control: "gain", value: -5 }));
    await act(async () => {
      await result.current.session.edit("setTrackMuted", [0, false]);
    });
    await backend.deliverAll();
    expect(gainOf(result.current.session)).toBe(-5);
    // An input that was not committed has not been sent.
    expect(result.current.session.sentValue(0, "gain")).toBeUndefined();

    act(() => result.current.session.dropInput(0, "gain"));
    expect(gainOf(result.current.session)).toBe(0);
  });

  it("holds nothing for a track that is not there", async () => {
    const { result } = await mount();
    act(() => result.current.session.holdInput({ index: 5, control: "gain", value: -5 }));
    act(() => result.current.session.dropInput(5, "gain"));
    expect(result.current.session.sentValue(5, "gain")).toBeUndefined();
    await act(async () => {
      await result.current.session.edit("setTrackGain", [0, -1], {
        hold: { index: 5, control: "gain", value: -1 },
      });
    });
    await backend.deliverAll();
    expect(gainOf(result.current.session)).toBe(-1);
  });
});

describe("a move the session makes by itself", () => {
  it("lets go of what the user holds, and reads the tracks again, for undo", async () => {
    const { result } = await mount();
    await act(async () => {
      await result.current.session.edit("setTrackMuted", [0, true]);
    });
    await backend.deliverAll();
    act(() => result.current.session.holdInput({ index: 0, control: "gain", value: -5 }));
    const reads = backend.count("list_tracks");

    await act(async () => {
      expect(await result.current.session.move({ kind: "undo" })).toBe(true);
    });
    await backend.deliverAll();

    expect(gainOf(result.current.session)).toBe(0);
    expect(backend.count("list_tracks")).toBeGreaterThan(reads);
  });

  it.each([
    ["redo", { kind: "redo" } as const],
    ["Set as head", { kind: "setHead", node: B } as const],
    ["Accept B", { kind: "acceptB", b: B } as const],
  ])("%s lets go of what the user holds", async (_, request) => {
    const { result } = await mount();
    await act(async () => {
      await result.current.session.edit("setTrackMuted", [0, true]);
    });
    await act(async () => {
      await result.current.session.move({ kind: "undo" });
    });
    act(() => result.current.session.holdInput({ index: 0, control: "gain", value: -5 }));

    await act(async () => {
      await result.current.session.move(request);
    });
    await backend.deliverAll();

    expect(gainOf(result.current.session)).toBe(backend.gain);
  });

  it("undo and redo with nothing to go to move nothing", async () => {
    const { result } = await mount();
    await act(async () => {
      expect(await result.current.session.move({ kind: "redo" })).toBe(false);
    });
    // The head A has no parent: nothing to undo either.
    await act(async () => {
      expect(await result.current.session.move({ kind: "undo" })).toBe(false);
    });
    expect(backend.count("set_head_to")).toBe(0);
  });

  it("a node the assistant creates lets go too, and the head follows it", async () => {
    const { result } = await mount();
    act(() => result.current.session.holdInput({ index: 0, control: "gain", value: -5 }));
    // Setting a draft draws nothing (the slider shows its own value); the
    // next list that arrives is drawn with it over it.
    await act(async () => {
      await result.current.session.edit("setTrackMuted", [0, false]);
    });
    await backend.deliverAll();
    expect(gainOf(result.current.session)).toBe(-5);
    const reads = backend.count("list_tracks");
    const edition = result.current.session.edition;

    backend.head = C;
    await act(async () => {
      await emit("agent://node-created", { node_id: C });
    });
    await backend.deliverAll();

    expect(result.current.session.head).toBe(C);
    expect(gainOf(result.current.session)).toBe(0);
    expect(backend.count("list_tracks")).toBe(reads + 1);
    expect(result.current.session.edition).toBeGreaterThan(edition);
  });

  it("opens a project: restores the view in the turn, resets the head with the saved path", async () => {
    const { result } = await mount();
    const restore = vi.fn(async () => ({ head: B, trail: [A], redo: [] }));
    backend.head = B;

    let info: unknown;
    await act(async () => {
      info = await result.current.session.move({ kind: "open", path: "/tmp/p", restore });
    });

    expect(restore).toHaveBeenCalledTimes(1);
    expect(info).toEqual({ head: B });
    expect(result.current.session.head).toBe(B);
    expect(result.current.session.trail).toEqual([A]);
  });

  it("Save As carries on in the copy and lets go of what is held", async () => {
    const { result } = await mount();
    act(() => result.current.session.holdInput({ index: 0, control: "gain", value: -5 }));
    const adopted = vi.fn();

    let report: unknown;
    await act(async () => {
      report = await result.current.session.move({ kind: "saveAs", dest: "/tmp/copy", adopted });
    });
    await backend.deliverAll();

    expect(report).toEqual({ files: 1, bytes: 1, skipped_cache_files: 0 });
    expect(adopted).toHaveBeenCalledTimes(1);
    expect(gainOf(result.current.session)).toBe(0);
  });

  it("a punch-in's reconcile adopts the head it left, and keeps another project's controls", async () => {
    const { result } = await mount();
    backend.head = C;
    let head: unknown;
    await act(async () => {
      head = await result.current.session.move({ kind: "reconcile", isCurrent: () => true });
    });
    expect(head).toBe(C);
    expect(result.current.session.head).toBe(C);

    // Another project is open now: what the user set there stays theirs.
    await act(async () => {
      await result.current.session.edit("setTrackMuted", [0, false]);
    });
    await backend.deliverAll();
    act(() => result.current.session.holdInput({ index: 0, control: "gain", value: -5 }));
    await act(async () => {
      await result.current.session.move({ kind: "reconcile", isCurrent: () => false });
    });
    await backend.deliverAll();
    expect(gainOf(result.current.session)).toBe(-5);
  });
});

describe("undo and redo skip the steps a compaction removed (#595)", () => {
  it("drops a step the store refuses, and goes on to the next", async () => {
    const { result } = await mount();
    await act(async () => {
      await result.current.session.edit("setTrackGain", [0, -1]);
    });
    const afterFirst = backend.head;
    await act(async () => {
      await result.current.session.edit("setTrackGain", [0, -2]);
    });
    // The store lost the step before the head, as a compaction does.
    backend.nodes = backend.nodes.filter((id) => id !== afterFirst);

    let moved = false;
    await act(async () => {
      moved = await result.current.session.move({ kind: "undo" });
    });

    expect(moved).toBe(true);
    // It tried the removed step, was refused, forgot it and took the next.
    expect(backend.sent.filter((s) => s.cmd === "set_head_to").map((s) => s.args.nodeId)).toEqual([
      afterFirst,
      A,
    ]);
    expect(result.current.session.head).toBe(A);
  });

  it("redo skips a removed step too, and reports a refusal it cannot skip", async () => {
    const { result } = await mount();
    await act(async () => {
      await result.current.session.edit("setTrackGain", [0, -1]);
    });
    const edited = backend.head;
    await act(async () => {
      await result.current.session.move({ kind: "undo" });
    });
    backend.nodes = backend.nodes.filter((id) => id !== edited);

    let moved: boolean | undefined;
    await act(async () => {
      moved = await result.current.session.move({ kind: "redo" });
    });
    // The only step to redo is gone: forgotten, and nothing left to redo.
    expect(moved).toBe(false);
    expect(result.current.session.head).toBe(A);
  });

  it("a redo the store refuses for a node it still has is an error, and the step is kept", async () => {
    const { result } = await mount();
    await act(async () => {
      await result.current.session.edit("setTrackGain", [0, -1]);
    });
    const edited = backend.head;
    await act(async () => {
      await result.current.session.move({ kind: "undo" });
    });
    backend.refuse.set("set_head_to", "the store is busy");

    let failure: unknown;
    await act(async () => {
      await result.current.session.move({ kind: "redo" }).catch((error: unknown) => (failure = error));
    });

    expect(String(failure)).toContain("the store is busy");
    expect(result.current.session.head).toBe(A);
    // Still there to redo once the store will take it.
    backend.refuse.clear();
    await act(async () => {
      expect(await result.current.session.move({ kind: "redo" })).toBe(true);
    });
    expect(result.current.session.head).toBe(edited);
  });

  it("is refused, and says so, for a node that is not gone", async () => {
    const { result } = await mount();
    await act(async () => {
      await result.current.session.edit("setTrackGain", [0, -1]);
    });
    backend.nodes = backend.nodes.filter((id) => id === A);

    let failure: unknown;
    await act(async () => {
      await result.current.session
        .move({ kind: "setHead", node: GONE })
        .catch((error: unknown) => (failure = error));
    });

    expect(String(failure)).toContain("node not found");
  });

  it("forgets what a compaction deleted when the agent's call ends", async () => {
    const { result } = await mount();
    await act(async () => {
      await result.current.session.edit("setTrackGain", [0, -1]);
    });
    await act(async () => {
      await result.current.session.edit("setTrackGain", [0, -2]);
    });
    backend.nodes = [A, backend.head];
    const graphReads = backend.count("get_graph");

    await act(async () => {
      await emit("agent://tool-call", { name: "compact_session", id: "t1" });
      await emit("agent://tool-call-end", { id: "t1", ok: true, not_run: false });
    });
    await waitFor(() => expect(backend.count("get_graph")).toBeGreaterThan(graphReads));

    // The path named a node that is gone: undo is not pointed at it.
    await waitFor(() => expect(result.current.session.trail).toEqual([A]));
  });

  it("asks nothing for another tool, or a compaction that failed", async () => {
    await mount();
    const graphReads = backend.count("get_graph");
    await act(async () => {
      await emit("agent://tool-call", { name: "storage_report", id: "t1" });
      await emit("agent://tool-call-end", { id: "t1", ok: true, not_run: false });
      await emit("agent://tool-call", { name: "compact_session", id: "t2" });
      await emit("agent://tool-call-end", { id: "t2", ok: false, not_run: false });
    });
    await settle();
    expect(backend.count("get_graph")).toBe(graphReads);
  });
});

describe("what a turn reads again, from the table", () => {
  /** How many times each read has been asked for. */
  const reads = () => ({
    tracks: backend.count("list_tracks"),
    graph: backend.count("get_graph"),
    markers: backend.count("list_markers"),
    transcript: backend.count("get_transcript"),
  });

  it("a head-moving edit reads the tracks, graph, labels and transcript once each", async () => {
    const { result } = await mount();
    const before = reads();

    await act(async () => {
      await result.current.session.edit("addMarker", [1, "verse"]);
    });
    await backend.deliverAll();
    await settle();

    const after = reads();
    expect(after.tracks - before.tracks).toBe(1);
    expect(after.graph - before.graph).toBe(1);
    expect(after.markers - before.markers).toBe(1);
    expect(after.transcript - before.transcript).toBe(1);
  });

  it("an undo reads them all again too: it can take a label back (#453)", async () => {
    const { result } = await mount();
    await act(async () => {
      await result.current.session.edit("addMarker", [1, "verse"]);
    });
    await backend.deliverAll();
    await settle();
    const before = reads();

    await act(async () => {
      await result.current.session.move({ kind: "undo" });
    });
    await backend.deliverAll();
    await settle();

    const after = reads();
    expect(after.tracks - before.tracks).toBe(1);
    expect(after.graph - before.graph).toBe(1);
    expect(after.markers - before.markers).toBe(1);
    expect(after.transcript - before.transcript).toBe(1);
  });

  it("naming a node reads the graph and nothing else", async () => {
    const { result } = await mount();
    const before = reads();

    await act(async () => {
      await result.current.session.edit("renameNode", [A, "intro"]);
    });
    await settle();

    const after = reads();
    expect(after.graph - before.graph).toBe(1);
    expect(after.tracks).toBe(before.tracks);
    expect(after.markers).toBe(before.markers);
    expect(after.transcript).toBe(before.transcript);
  });

  it("the end of the agent's turn reads the graph: a label moves no head (#601)", async () => {
    await mount();
    const before = reads();

    await act(async () => {
      await emit("agent://done", {});
    });
    await settle();

    const after = reads();
    expect(after.graph - before.graph).toBe(1);
    expect(after.tracks).toBe(before.tracks);
  });

  it("a label event reads the labels and nothing else", async () => {
    await mount();
    const before = reads();

    await act(async () => {
      await emit("marker-changed", {});
    });
    await settle();

    const after = reads();
    expect(after.markers - before.markers).toBe(1);
    expect(after.graph).toBe(before.graph);
    expect(after.tracks).toBe(before.tracks);
  });

  it("a refused edit is read back too, and a failed track read is reported", async () => {
    const { result } = await mount();
    const before = reads();
    backend.refuse.set("set_track_gain", "out of range");
    const original = backend.handle;
    backend.handle = (cmd, args) => {
      if (cmd === "list_tracks") {
        backend.calls++;
        backend.sent.push({ cmd, args: args ?? {} });
        throw "the session could not be read";
      }
      return original(cmd, args);
    };
    bootIpc(backend);

    await act(async () => {
      await result.current.session.edit("setTrackGain", [0, 99]).catch(() => undefined);
    });
    await settle();

    expect(reads().tracks - before.tracks).toBe(1);
    expect(readError.map(String)).toEqual(["the session could not be read"]);
  });
});

describe("the transcript read", () => {
  it("is no transcript when the session cannot answer", async () => {
    const original = backend.handle;
    let asked = 0;
    backend.handle = (cmd, args) => {
      if (cmd === "get_transcript") {
        asked++;
        throw "no session loaded; call open_project first";
      }
      return original(cmd, args);
    };
    bootIpc(backend);
    const { result } = await mount();
    await waitFor(() => expect(asked).toBeGreaterThan(0));
    await settle();
    expect(result.current.transcript).toEqual({ words: [], source: { kind: "unknown" } });
  });
});

describe("the track read", () => {
  it("is an empty list for a project with nothing in it yet", async () => {
    const original = backend.handle;
    backend.handle = (cmd, args) => {
      if (cmd === "list_tracks") throw "no session loaded; call open_project first";
      return original(cmd, args);
    };
    bootIpc(backend);
    const hook = renderHook(useWindow, { wrapper: SessionQueryProvider });
    await waitFor(() => expect(hook.result.current.session.head).toBe(A));
    await settle();
    expect(hook.result.current.session.sessionTracks).toEqual([]);
    expect(readError).toEqual([]);
  });

  it("hands out new references for identical data, so a lane draws the session's value again", async () => {
    const { result } = await mount();
    const before = result.current.session.sessionTracks;
    await act(async () => {
      await result.current.session.edit("setTrackMuted", [0, false]);
    });
    await backend.deliverAll();
    const after = result.current.session.sessionTracks;
    expect(after).toEqual(before);
    expect(after).not.toBe(before);
    expect(after[0]).not.toBe(before[0]);
    expect(after[0].clips).not.toBe(before[0].clips);
  });

  it("is not answered by a read asked for before the assistant's node arrived", async () => {
    // The boot read is on its way when the node arrives: it was asked for
    // before the move, so it must not stand as the session's list.
    backend.holdReads = true;
    const hook = renderHook(useWindow, { wrapper: SessionQueryProvider });
    await waitFor(() => expect(hook.result.current.session.head).toBe(A));
    await waitFor(() => expect(backend.pendingReads).toHaveLength(1));
    const boot = backend.pendingReads[0];

    backend.gain = -6;
    backend.head = C;
    await act(async () => {
      await emit("agent://node-created", { node_id: C });
    });
    await waitFor(() => expect(backend.count("list_tracks")).toBe(2));
    const newer = backend.pendingReads[backend.pendingReads.length - 1];
    expect(newer).not.toBe(boot);

    await backend.deliver(newer);
    await waitFor(() => expect(gainOf(hook.result.current.session)).toBe(-6));
    await backend.deliver(boot);
    await backend.deliverAll();
    expect(gainOf(hook.result.current.session)).toBe(-6);
  });

  it("works while the query online manager says offline: these are local commands", async () => {
    const wasOnline = onlineManager.isOnline();
    onlineManager.setOnline(false);
    try {
      const { result } = await mount();
      await act(async () => {
        await result.current.session.edit("setTrackGain", [0, -3]);
      });
      await backend.deliverAll();
      expect(result.current.session.sessionTracks[0].gain_db).toBe(-3);
    } finally {
      onlineManager.setOnline(wasOnline);
    }
  });

  it("is private to each mounted app", async () => {
    const first = await mount();
    await act(async () => {
      await first.result.current.session.edit("setTrackGain", [0, -3]);
    });
    await backend.deliverAll();
    const second = renderHook(useWindow, {
      wrapper: ({ children }: { children: ReactNode }) => (
        <QueryClientProvider client={createSessionQueryClient()}>{children}</QueryClientProvider>
      ),
    });
    await waitFor(() => expect(second.result.current.session.sessionTracks).toHaveLength(1));
    expect(second.result.current.session.sessionTracks[0].gain_db).toBe(-3);

    // The session changes under the first app only; the second keeps what
    // it read until it is told.
    backend.gain = 0;
    await act(async () => {
      await first.result.current.session.edit("setTrackMuted", [0, true]);
    });
    await backend.deliverAll();
    expect(first.result.current.session.sessionTracks[0].gain_db).toBe(0);
    expect(second.result.current.session.sessionTracks[0].gain_db).toBe(-3);
  });

  it("listens once, and stops listening on unmount, under StrictMode", async () => {
    const hook = renderHook(useWindow, {
      wrapper: ({ children }: { children: ReactNode }) => (
        <StrictMode>
          <SessionQueryProvider>{children}</SessionQueryProvider>
        </StrictMode>
      ),
    });
    await waitFor(() => expect(hook.result.current.session.head).toBe(A));
    await settle();
    const reads = backend.count("list_tracks");

    backend.head = C;
    await act(async () => {
      await emit("agent://node-created", { node_id: C });
    });
    await settle();
    // One node, one read: a listener left behind by the first mount would read twice.
    expect(backend.count("list_tracks") - reads).toBe(1);
    hook.unmount();
  });
});
