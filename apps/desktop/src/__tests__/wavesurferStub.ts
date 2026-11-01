/**
 * A WaveSurfer instance whose methods do nothing and whose duration is ten
 * seconds, for the tests that change only a member or two of it
 * (`Timeline.*.test.tsx`). `wavesurferMock.ts` is the unchanged stub the
 * whole-`App` tests share; this builds one with `over` laid on top of it.
 *
 *   vi.mock("wavesurfer.js", async () =>
 *     (await import("./wavesurferStub")).wavesurferWith(() => ({ getDuration: () => 60 })),
 *   );
 *
 * `over` is called once for each instance, so a `vi.fn()` it makes belongs
 * to that instance; a mock it names from `vi.hoisted` is shared.
 */

import { vi } from "vitest";

import { wavesurferModule } from "./wavesurferMock";

type Members = Record<string, unknown>;

/** The instance: every member a no-op, `over` replacing the ones a test needs. */
export function wavesurferInstance(over: Members = {}) {
  return { ...wavesurferModule().default.create(), ...over };
}

/** The module the factory of `vi.mock("wavesurfer.js")` returns. */
export function wavesurferWith(over: () => Members) {
  return { default: { create: () => wavesurferInstance(over()) } };
}

/** `on` for an instance that has already decoded: `decode` fires at once. */
export const onDecode = () =>
  vi.fn((event: string, cb: () => void) => {
    if (event === "decode") cb();
  });

/** What the lane draws at the timeline's density and scrolls through (#344). */
export const laneMembers = (): Members => ({
  getWrapper: () => document.createElement("div"),
  setScroll: vi.fn(),
  getScroll: () => 0,
});

/** One player `eventedPlayer` made: what it holds and how to drive it. */
export interface PlayerEntry {
  url: string | null;
  currentTime: number;
  playing: boolean;
  play: ReturnType<typeof vi.fn>;
  pause: ReturnType<typeof vi.fn>;
  setTime: ReturnType<typeof vi.fn>;
  /** Fire one of the player's events, as its media element would. */
  emit: (event: string, ...args: unknown[]) => void;
}

/**
 * A player that reports what the real one does: `play` and `pause` come
 * back as events, `setTime` as a `timeupdate`, a new source zeroes the
 * position, and nothing loaded has no duration, which is what makes a seek
 * or a play against a cold start a no-op in the real thing. Each one made
 * is pushed on `instances`; `answerLoad` says how a load is answered.
 */
export function eventedPlayer(
  instances: { push(entry: PlayerEntry): unknown },
  answerLoad: (url: string) => Promise<void> = () => Promise.resolve(),
) {
  type Handler = (...args: unknown[]) => void;
  const handlers = new Map<string, Set<Handler>>();
  const emit = (event: string, ...args: unknown[]) => {
    for (const cb of handlers.get(event) ?? []) cb(...args);
  };
  const entry: PlayerEntry = {
    url: null,
    currentTime: 0,
    playing: false,
    play: vi.fn(() => {
      entry.playing = true;
      emit("play");
      return Promise.resolve();
    }),
    pause: vi.fn(() => {
      entry.playing = false;
      emit("pause");
    }),
    setTime: vi.fn((t: number) => {
      entry.currentTime = t;
      emit("timeupdate", t);
    }),
    emit,
  };
  instances.push(entry);
  return wavesurferInstance({
    on: vi.fn((event: string, cb: Handler) => {
      if (!handlers.has(event)) handlers.set(event, new Set());
      handlers.get(event)!.add(cb);
      if (event === "decode") cb();
    }),
    un: vi.fn((event: string, cb: Handler) => {
      handlers.get(event)?.delete(cb);
    }),
    load: vi.fn((url: string) => {
      entry.url = url;
      entry.currentTime = 0;
      entry.playing = false;
      return answerLoad(url);
    }),
    play: entry.play,
    pause: entry.pause,
    setTime: entry.setTime,
    isPlaying: () => entry.playing,
    getDuration: () => (entry.url ? 60 : 0),
    getCurrentTime: () => entry.currentTime,
    ...laneMembers(),
  });
}
