/**
 * The path is saved without anything waiting for the save (#453), and the
 * file ends up holding the newest path however fast the user edits.
 */

import { describe, expect, it } from "vitest";

import type { SavedTrail } from "../headTrail";
import { createTrailWriter } from "../trailWriter";

const path = (head: string): SavedTrail => ({ head, trail: [], redo: [] });

/** A backend whose saves are answered when the test chooses. */
function backend() {
  const sent: SavedTrail[] = [];
  const answers: { ok: () => void; fail: (why: string) => void }[] = [];
  let inFlight = 0;
  let mostInFlight = 0;
  return {
    sent,
    get mostInFlight() {
      return mostInFlight;
    },
    get pending() {
      return answers.length;
    },
    send: (trail: SavedTrail) => {
      sent.push(trail);
      mostInFlight = Math.max(mostInFlight, ++inFlight);
      return new Promise<void>((resolve, reject) => {
        answers.push({
          ok: () => {
            inFlight--;
            resolve();
          },
          fail: (why) => {
            inFlight--;
            reject(why);
          },
        });
      });
    },
    /** Answer the oldest save not yet answered, and let the writer go on. */
    async answer(how: "ok" | "fail" = "ok") {
      const next = answers.shift();
      if (!next) throw new Error("nothing is waiting for an answer");
      if (how === "ok") next.ok();
      else next.fail("disk full");
      await flush();
    },
  };
}

/** Let every promise already settled run what follows it. */
const flush = async () => {
  for (let i = 0; i < 10; i++) await Promise.resolve();
};

describe("the trail writer", () => {
  it("sends a save at once, and the caller does not wait for the answer", async () => {
    const be = backend();
    const writer = createTrailWriter(be.send);

    const returned = writer.save(path("A"));
    expect(returned).toBeUndefined();
    await flush();

    // Sent, and still unanswered: the call returned long before that.
    expect(be.sent).toEqual([path("A")]);
  });

  it("sends nothing while a save is in flight, then only the newest", async () => {
    const be = backend();
    const writer = createTrailWriter(be.send);

    writer.save(path("A"));
    await flush();
    writer.save(path("B"));
    writer.save(path("C"));
    writer.save(path("D"));
    await flush();
    expect(be.sent).toEqual([path("A")]);

    await be.answer();
    // B and C were written over by D before they were ever sent.
    expect(be.sent).toEqual([path("A"), path("D")]);

    await be.answer();
    expect(be.sent).toEqual([path("A"), path("D")]);
  });

  it("never has two saves in flight, so the older cannot land last", async () => {
    const be = backend();
    const writer = createTrailWriter(be.send);

    for (let i = 0; i < 20; i++) {
      writer.save(path(`n${i}`));
      await flush();
      if (i % 3 === 0) await be.answer();
    }
    while (be.pending > 0) await be.answer();

    expect(be.mostInFlight).toBe(1);
    // And what was sent last is the newest path there was.
    expect(be.sent[be.sent.length - 1]).toEqual(path("n19"));
    // In order: no save is sent after one that was made later.
    const heads = be.sent.map((p) => Number(p.head!.slice(1)));
    expect(heads).toEqual([...heads].sort((a, b) => a - b));
  });

  it("a save that fails does not stop the ones behind it", async () => {
    const be = backend();
    const writer = createTrailWriter(be.send);

    writer.save(path("A"));
    await flush();
    writer.save(path("B"));
    await be.answer("fail");

    expect(be.sent).toEqual([path("A"), path("B")]);
  });

  it("a send that throws is a failed save, not an error for the caller", async () => {
    const sent: string[] = [];
    let refuse = true;
    const writer = createTrailWriter((trail) => {
      sent.push(trail.head!);
      if (refuse) throw new Error("no such command");
      return Promise.resolve();
    });

    expect(() => writer.save(path("A"))).not.toThrow();
    await flush();
    refuse = false;
    writer.save(path("B"));
    await flush();

    expect(sent).toEqual(["A", "B"]);
  });

  it("forgets a save still waiting, but not the one in flight", async () => {
    const be = backend();
    const writer = createTrailWriter(be.send);

    writer.save(path("A"));
    await flush();
    writer.save(path("B")); // behind A: the old project's path
    writer.forget();
    await be.answer();

    expect(be.sent).toEqual([path("A")]);

    // And it goes on: the next project's first save is sent.
    writer.save(path("X"));
    await flush();
    expect(be.sent).toEqual([path("A"), path("X")]);
  });
});
