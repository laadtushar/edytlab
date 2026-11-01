/**
 * What the whole-`App` tests share (`App.*.test.tsx`): the real `App` runs
 * with only the IPC boundary replaced, and each test supplies its own
 * `Backend` class with the commands it cares about. This file holds the
 * rest: the answers every backend gives alike, the track every one lists,
 * and the waiting that follows a boot.
 *
 *   vi.mock("wavesurfer.js", async () => (await import("./wavesurferMock")).wavesurferModule());
 *
 *   handle = (cmd: string, args: Args = {}): unknown => {
 *     switch (cmd) {
 *       case "set_head_to": ...
 *       default:
 *         return appAnswer(cmd);
 *     }
 *   };
 */

import { act } from "@testing-library/react";
import { mockIPC } from "@tauri-apps/api/mocks";

import { held, type Held } from "./held";

export type Args = Record<string, unknown>;

/** A node id; the status bar shows the first seven characters. */
export const node = (n: number): string => n.toString(16).padStart(7, "0").padEnd(64, "0");

/** The id of the track `voiceTrack` lists. */
export const VOICE_ID = "3f2b8c1e-7d4a-4e9b-9c6f-1a2b3c4d5e6f";

/** The one-clip voice track the tests list; `over` changes what a test varies. */
export function voiceTrack(over: Record<string, unknown> = {}) {
  return {
    id: VOICE_ID,
    name: "Voice",
    muted: false,
    gain_db: 0,
    pan: 0,
    soloed: false,
    audio_path: "/tmp/voice.wav",
    clips: [{ start_sec: 0, length_sec: 10, source_path: "/tmp/voice.wav", volume_envelope: [] }],
    ...over,
  };
}

/**
 * What the app reads while it boots, answered the way an idle project
 * would: a key is saved, the keychain is durable, no templates or recent
 * projects. Every other command answers `undefined`.
 */
export function startupAnswer(cmd: string): unknown {
  switch (cmd) {
    case "has_api_key":
      return true;
    case "get_keychain_persistence":
      return { persistent: true, reason: null };
    case "install_bundled_skills":
      return 0;
    case "list_templates":
    case "list_recent_projects":
      return [];
    default:
      return undefined;
  }
}

/**
 * `startupAnswer`, and the reads and writes that follow once a head is
 * there (markers, transcript, view state, the two toggles): the fallback
 * of a backend with nothing to say about them.
 */
export function appAnswer(cmd: string): unknown {
  switch (cmd) {
    case "list_markers":
      return [];
    case "get_transcript":
      return { words: [], source: { kind: "unknown" } };
    case "get_view_state":
      return {};
    case "get_plan_first":
    case "get_sync_lock":
      return false;
    case "save_view_state":
    case "set_selection_context":
      return null;
    default:
      return startupAnswer(cmd);
  }
}

/** Put `backend` behind the IPC boundary, with events mocked. */
export function bootIpc(backend: { handle(cmd: string, args?: Args): unknown }): void {
  mockIPC((cmd, args) => backend.handle(cmd, args as Args | undefined), {
    shouldMockEvents: true,
  });
}

/** Let what the app does next happen. */
export const settle = () =>
  act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 0));
  });

/**
 * Wait until the app has stopped asking for things.
 *
 * Boot is not over when the first lane is drawn: the head arriving makes
 * the app read the transcript, the labels and the sync-lock, and each
 * answer re-renders it. The timeline puts its lanes back to what the
 * session holds whenever it is handed a new track list, which every
 * render of the app does, so a fader dragged just as one of those lands
 * is put back before it is let go of, and the drag is lost.
 */
export async function quiet(backend: { calls: number }): Promise<void> {
  for (let guard = 0; guard < 20; guard++) {
    const before = backend.calls;
    await settle();
    if (backend.calls === before) return;
  }
  throw new Error("the app never stopped asking");
}

/** A track list read the backend has been asked for and has not answered. */
export interface Read {
  /** The list as it was when the read was made. */
  snapshot: unknown;
  reply: Held<unknown>;
}

/**
 * A backend that can hold the answer to a track-list read until the test
 * delivers it (`held.ts`), with the list as it was when the read was made:
 * `App.laneControls.test.tsx` and `App.sessionMoves.test.tsx`. Its
 * `list_tracks` case returns `this.heldRead(list)`.
 */
export class HeldTrackReads {
  /** Answer a track-list read when the test delivers it, not at once. */
  holdReads = false;
  private reads: Read[] = [];

  /** The reads waiting for an answer, oldest first. */
  get pendingReads(): readonly Read[] {
    return this.reads;
  }

  /** The answer to a track-list read: `snapshot` now, or when delivered. */
  protected heldRead(snapshot: unknown): unknown {
    if (!this.holdReads) return snapshot;
    const read: Read = { snapshot, reply: held<unknown>() };
    this.reads.push(read);
    return read.reply.promise;
  }

  /** Deliver a held read: the list as it was when it was asked for. */
  async deliver(read: Read): Promise<void> {
    this.reads = this.reads.filter((r) => r !== read);
    await read.reply.resolve(read.snapshot);
  }

  /** Deliver every held read, oldest first, until none is left. */
  async deliverAll(): Promise<void> {
    for (let guard = 0; guard < 20; guard++) {
      await settle();
      if (this.reads.length === 0) return;
      await this.deliver(this.reads[0]);
    }
    throw new Error("the app kept asking for the tracks");
  }
}

/**
 * React Flow measures what it draws; jsdom has neither of these. Call it
 * from `beforeEach` in a test that draws the history graph.
 */
export function stubGraphGlobals(): void {
  if (!("ResizeObserver" in window)) {
    class RO {
      observe(): void {}
      unobserve(): void {}
      disconnect(): void {}
    }
    (window as unknown as { ResizeObserver: typeof RO }).ResizeObserver = RO;
  }
  if (!("DOMMatrixReadOnly" in window)) {
    class M {
      m22 = 1;
      constructor(_init?: string | number[]) {}
    }
    (window as unknown as { DOMMatrixReadOnly: typeof M }).DOMMatrixReadOnly = M;
  }
}
