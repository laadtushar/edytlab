/**
 * The overlay of the controls a user holds over the session's track list
 * (`lib/heldControls.ts`, #596): the pure part. When a hold begins and ends
 * is `useSession`'s (`useSession.session.test.tsx`).
 */

import { describe, expect, it } from "vitest";

import { lastHeld, overlay, trackKey, type Hold } from "../lib/heldControls";
import type { TrackSummary } from "../lib/tauri-bridge";

const track = (id: string | undefined, over: Partial<TrackSummary> = {}): TrackSummary =>
  ({
    id: id as string,
    name: "Voice",
    muted: false,
    gain_db: 0,
    pan: 0,
    soloed: false,
    audio_path: "/tmp/voice.wav",
    clips: [],
    ...over,
  }) as TrackSummary;

const hold = (trackId: string, control: Hold["control"], value: Hold["value"]): Hold => ({
  track: trackId,
  control,
  value,
});

describe("overlay", () => {
  it("is the list itself when nothing is held, so an untouched list keeps its identity", () => {
    const tracks = [track("a")];
    expect(overlay(tracks, [])).toBe(tracks);
  });

  it("puts the user's value over the session's, for the control they hold only", () => {
    const tracks = [track("a", { gain_db: 6, pan: 0.5 })];
    const drawn = overlay(tracks, [hold("a", "gain", -3)]);
    expect(drawn[0]).toMatchObject({ gain_db: -3, pan: 0.5, muted: false });
    // The session's own track is not changed.
    expect(tracks[0].gain_db).toBe(6);
  });

  it("holds every kind of control", () => {
    const drawn = overlay(
      [track("a")],
      [hold("a", "pan", -0.5), hold("a", "muted", true), hold("a", "soloed", true)],
    );
    expect(drawn[0]).toMatchObject({ pan: -0.5, muted: true, soloed: true });
  });

  it("the newest hold of a control wins", () => {
    const drawn = overlay([track("a")], [hold("a", "gain", -3), hold("a", "gain", 2)]);
    expect(drawn[0].gain_db).toBe(2);
  });

  it("follows the track it was set on when positions change", () => {
    const drawn = overlay([track("b"), track("a")], [hold("a", "gain", -3)]);
    expect(drawn.map((t) => t.gain_db)).toEqual([0, -3]);
  });

  it("leaves a track the session already agrees with as it is", () => {
    const tracks = [track("a", { gain_db: -3 })];
    // The session stores f32: a value within the tolerance is the same one.
    const drawn = overlay(tracks, [hold("a", "gain", -3.00001)]);
    expect(drawn[0]).toBe(tracks[0]);
  });
});

describe("trackKey", () => {
  it("is the track's id, and its position for a track with none", () => {
    expect(trackKey([track("a"), track(undefined)], 0)).toBe("a");
    expect(trackKey([track("a"), track(undefined)], 1)).toBe("#1");
    expect(trackKey([track("a")], 3)).toBeNull();
  });
});

describe("lastHeld", () => {
  it("is the newest value held for the control, or nothing", () => {
    const holds = [hold("a", "gain", -3), hold("b", "gain", 1), hold("a", "gain", 2)];
    expect(lastHeld(holds, "a", "gain")).toBe(2);
    expect(lastHeld(holds, "a", "pan")).toBeUndefined();
  });
});
