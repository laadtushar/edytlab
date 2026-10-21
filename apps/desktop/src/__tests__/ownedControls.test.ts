import { describe, expect, it, vi } from "vitest";

import { OwnedControls, trackKey } from "../lib/ownedControls";
import type { TrackSummary } from "../lib/tauri-bridge";

/**
 * The hand-over rule behind a lane control that survives the read-back of
 * an earlier commit (#596). `App.laneControls.test.tsx` runs it through the
 * real app; this is the rule on its own, step by step.
 */

const track = (id: string, over: Partial<TrackSummary> = {}): TrackSummary => ({
  id,
  name: id,
  muted: false,
  gain_db: 0,
  pan: 0,
  soloed: false,
  audio_path: `/tmp/${id}.wav`,
  clips: [],
  ...over,
});

function make() {
  const changed = vi.fn();
  return { owned: new OwnedControls(changed), changed };
}

describe("a control the user has set", () => {
  it("is laid over a list that carries an older value, and only that control", () => {
    const { owned } = make();
    owned.set("a", "gain", 1);

    const [a, b] = owned.overlay([track("a", { gain_db: 0.5, pan: 0.2 }), track("b")]);

    expect(a.gain_db).toBe(1);
    expect(a.pan).toBe(0.2);
    expect(b.gain_db).toBe(0);
  });

  it("is held for the track it was set on, wherever that track now is in the list", () => {
    const { owned } = make();
    owned.set("b", "muted", true);

    const [first, second] = owned.overlay([track("b"), track("a")]);

    expect(first.muted).toBe(true);
    expect(second.muted).toBe(false);
  });

  it("leaves the list itself alone when nothing is held", () => {
    const { owned } = make();
    const list = [track("a")];
    expect(owned.overlay(list)).toBe(list);
  });

  it("does not ask for a redraw when it is set or sent: what it holds is already drawn", () => {
    const { owned, changed } = make();
    owned.set("a", "gain", 1);
    owned.send("a", "gain", 1);
    expect(changed).not.toHaveBeenCalled();
  });

  it("is not handed back by a list while its value has not been sent", () => {
    const { owned, changed } = make();
    owned.set("a", "gain", 1);
    owned.applied(1);
    owned.applied(100);
    expect(owned.overlay([track("a")])[0].gain_db).toBe(1);
    expect(changed).not.toHaveBeenCalled();
  });
});

describe("a commit", () => {
  it("is not confirmed by a list read before it was answered, whenever that list lands", () => {
    const { owned, changed } = make();
    owned.set("a", "gain", 1);
    const claim = owned.send("a", "gain", 1);
    owned.finish(claim, 5);

    owned.applied(3);
    owned.applied(4);
    expect(owned.overlay([track("a")])[0].gain_db).toBe(1);
    expect(changed).not.toHaveBeenCalled();

    owned.applied(5);
    expect(owned.overlay([track("a")])[0].gain_db).toBe(0);
    expect(changed).toHaveBeenCalledTimes(1);
  });

  it("is confirmed by any later list as well, so a read that was overtaken leaves nothing held", () => {
    const { owned } = make();
    const claim = owned.send("a", "muted", true);
    owned.finish(claim, 5);

    owned.applied(7);

    expect(owned.overlay([track("a")])[0].muted).toBe(false);
  });

  it("is not confirmed while it is still on its way", () => {
    const { owned } = make();
    owned.send("a", "gain", 1);
    owned.applied(100);
    expect(owned.overlay([track("a")])[0].gain_db).toBe(1);
  });

  it("holds the control until every commit sent for it has been answered", () => {
    const { owned } = make();
    const first = owned.send("a", "muted", true);
    const second = owned.send("a", "muted", false);

    owned.finish(first, 5);
    owned.applied(5);
    // The second is not answered yet, and the user's value is the second's.
    expect(owned.overlay([track("a", { muted: true })])[0].muted).toBe(false);
    expect(owned.overlay([track("a", { muted: false })])[0].muted).toBe(false);

    owned.finish(second, 6);
    owned.applied(5);
    expect(owned.overlay([track("a", { muted: true })])[0].muted).toBe(false);
    owned.applied(6);
    expect(owned.overlay([track("a", { muted: true })])[0].muted).toBe(true);
  });

  it("is held again when the user sets the control after it was answered", () => {
    const { owned } = make();
    const claim = owned.send("a", "gain", 0.5);
    owned.finish(claim, 5);

    owned.set("a", "gain", 1);
    owned.applied(5);

    expect(owned.overlay([track("a", { gain_db: 0.5 })])[0].gain_db).toBe(1);
  });

  it("is a value to send again only when it differs from the last one sent", () => {
    const { owned } = make();
    expect(owned.sent("a", "gain")).toBeUndefined();

    owned.send("a", "gain", -3);
    expect(owned.sent("a", "gain")).toBe(-3);
  });

  it("keeps the last value sent until a list confirms it, not only while it is on its way", () => {
    const { owned } = make();
    const claim = owned.send("a", "gain", -3);
    owned.finish(claim, 5);

    // Answered, and the list read for it has not landed: a slider returning
    // to 0 must still be compared with -3, not with the 0 on screen (#498).
    expect(owned.sent("a", "gain")).toBe(-3);

    owned.applied(5);
    expect(owned.sent("a", "gain")).toBeUndefined();
  });

  it("compares a number as the session stores it, an f32", () => {
    const { owned } = make();
    owned.set("a", "pan", 0.02);
    const [drawn] = owned.overlay([track("a", { pan: 0.019999999552965164 })]);
    expect(drawn.pan).toBeCloseTo(0.02, 6);
  });
});

describe("a value that needed no commit", () => {
  it("hands the control back when nothing is on its way", () => {
    const { owned, changed } = make();
    owned.set("a", "gain", 1);

    owned.skip("a", "gain");

    expect(owned.overlay([track("a")])[0].gain_db).toBe(0);
    expect(changed).toHaveBeenCalledTimes(1);
  });

  it("keeps the control held when it is the value of a commit that is on its way", () => {
    const { owned } = make();
    const claim = owned.send("a", "gain", 1);
    owned.set("a", "gain", 1);

    owned.skip("a", "gain");
    owned.applied(100);
    expect(owned.overlay([track("a")])[0].gain_db).toBe(1);

    owned.finish(claim, 5);
    owned.applied(5);
    expect(owned.overlay([track("a")])[0].gain_db).toBe(0);
  });

  it("is nothing for a control that was never set", () => {
    const { owned, changed } = make();
    owned.skip("a", "gain");
    expect(changed).not.toHaveBeenCalled();
  });
});

describe("the session moving by itself", () => {
  it("hands every control back at once", () => {
    const { owned, changed } = make();
    owned.set("a", "gain", 1);
    owned.send("b", "muted", true);

    owned.clear();

    const [a, b] = owned.overlay([track("a"), track("b")]);
    expect(a.gain_db).toBe(0);
    expect(b.muted).toBe(false);
    expect(changed).toHaveBeenCalledTimes(1);
  });

  it("is not undone by the answer to a commit sent before it", () => {
    const { owned } = make();
    const before = owned.send("a", "gain", 1);
    owned.clear();

    owned.set("a", "gain", 2);
    const after = owned.send("a", "gain", 2);
    // The old commit is answered now. It must not count against the new one.
    owned.finish(before, 9);
    owned.applied(100);
    expect(owned.overlay([track("a")])[0].gain_db).toBe(2);

    owned.finish(after, 10);
    owned.applied(10);
    expect(owned.overlay([track("a")])[0].gain_db).toBe(0);
  });

  it("asks for no redraw when nothing was held", () => {
    const { owned, changed } = make();
    owned.clear();
    expect(changed).not.toHaveBeenCalled();
  });
});

describe("trackKey", () => {
  it("is the track's id, and null for a position with no track", () => {
    const list = [track("a"), track("b")];
    expect(trackKey(list, 1)).toBe("b");
    expect(trackKey(list, 2)).toBeNull();
  });
});
