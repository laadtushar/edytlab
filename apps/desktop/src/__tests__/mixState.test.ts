import { describe, expect, it } from "vitest";
import { mixIsStale, afterSessionAdvanced, NO_MIX } from "../lib/mixState";

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
