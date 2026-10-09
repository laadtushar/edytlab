import { describe, expect, it } from "vitest";
import {
  afterSessionAdvanced,
  mixIsCurrent,
  mixIsMissing,
  mixIsStale,
  NO_MIX,
} from "../lib/mixState";

describe("mix staleness", () => {
  it("a mix rendered from the current head is not stale", () => {
    expect(mixIsStale({ mixPath: "/tmp/p-abc.wav", mixNodeId: "abc" }, "abc")).toBe(
      false,
    );
  });

  it("a mix rendered from a different node is stale", () => {
    expect(mixIsStale({ mixPath: "/tmp/p-abc.wav", mixNodeId: "abc" }, "def")).toBe(
      true,
    );
  });

  /**
   * The distinction that motivated extracting this. Nothing rendered
   * means nothing to be out of date — reporting "stale" there would name
   * a state the user cannot act on, since there is no mix to refresh.
   */
  it("no mix at all is not stale", () => {
    expect(mixIsStale(NO_MIX, "abc")).toBe(false);
    expect(mixIsStale(NO_MIX, null)).toBe(false);
  });

  /**
   * The specific failure this guards. `render_preview` names its output
   * after the node id, so a stale path string is indistinguishable from
   * a current one — the node id is the only thing that separates them.
   */
  it("is decided by the node id, never by the path", () => {
    const sameLookingPath = "/tmp/edytlab-preview-abc.wav";
    expect(
      mixIsStale({ mixPath: sameLookingPath, mixNodeId: "abc" }, "abc"),
    ).toBe(false);
    expect(
      mixIsStale({ mixPath: sameLookingPath, mixNodeId: "abc" }, "zzz"),
    ).toBe(true);
  });

  it("a session with no head yet makes any existing mix stale", () => {
    expect(mixIsStale({ mixPath: "/tmp/p.wav", mixNodeId: "abc" }, null)).toBe(
      true,
    );
  });

  it("advancing the session clears the mix rather than keeping an old one", () => {
    expect(afterSessionAdvanced()).toEqual({ mixPath: null, mixNodeId: null });
    expect(mixIsStale(afterSessionAdvanced(), "abc")).toBe(false);
  });
});

/**
 * Play renders the head's preview unless the mix is current (#431).
 * Absent and stale are both "not current", and are told apart only by
 * what the status bar says about them.
 */
describe("a current mix", () => {
  it("is one rendered from the head", () => {
    expect(mixIsCurrent({ mixPath: "/tmp/p-abc.wav", mixNodeId: "abc" }, "abc")).toBe(true);
  });

  it("is not one rendered from another node", () => {
    expect(mixIsCurrent({ mixPath: "/tmp/p-abc.wav", mixNodeId: "abc" }, "def")).toBe(false);
  });

  it("is not no mix at all, which is not stale either", () => {
    expect(mixIsCurrent(NO_MIX, "abc")).toBe(false);
    expect(mixIsStale(NO_MIX, "abc")).toBe(false);
  });

  it("needs a head to be current of", () => {
    // Compare puts a path in with no node; with no head it matches nothing.
    expect(mixIsCurrent({ mixPath: "/tmp/p.wav", mixNodeId: null }, null)).toBe(false);
  });

  it("is never both current and stale", () => {
    const heads = [null, "abc", "def"];
    const mixes = [
      NO_MIX,
      { mixPath: "/tmp/p.wav", mixNodeId: "abc" },
      { mixPath: "/tmp/p.wav", mixNodeId: null },
    ];
    for (const mix of mixes) {
      for (const head of heads) {
        expect(mixIsCurrent(mix, head) && mixIsStale(mix, head)).toBe(false);
      }
    }
  });
});

describe("a missing mix", () => {
  it("is nothing rendered yet, and only that", () => {
    expect(mixIsMissing(NO_MIX)).toBe(true);
    expect(mixIsMissing({ mixPath: "/tmp/p.wav", mixNodeId: "abc" })).toBe(false);
    expect(mixIsMissing({ mixPath: "/tmp/p.wav", mixNodeId: null })).toBe(false);
  });

  it("is what an edit leaves", () => {
    expect(mixIsMissing(afterSessionAdvanced())).toBe(true);
  });
});
