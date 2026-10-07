import { describe, expect, it } from "vitest";
import { describeLoadFailures } from "../lib/load-failures";

describe("describeLoadFailures", () => {
  it("says nothing when nothing failed", () => {
    expect(describeLoadFailures([])).toBeNull();
  });

  it("names the one file and the tool's reason", () => {
    expect(
      describeLoadFailures([{ path: "/home/me/notes.wav", error: "decode failed: no format" }]),
    ).toBe("Could not load notes.wav: decode failed: no format");
  });

  it("takes the file name from a Windows path too", () => {
    expect(describeLoadFailures([{ path: "C:\\Users\\me\\take 2.wav", error: "gone" }])).toBe(
      "Could not load take 2.wav: gone",
    );
  });

  it("names every file when a few fail", () => {
    const msg = describeLoadFailures([
      { path: "/a/one.wav", error: "e1" },
      { path: "/a/two.wav", error: "e2" },
    ]);
    expect(msg).toBe("Could not load 2 files: one.wav (e1); two.wav (e2)");
  });

  it("counts the rest instead of listing every failure", () => {
    const failures = Array.from({ length: 5 }, (_, i) => ({ path: `/a/f${i}.wav`, error: "bad" }));
    const msg = describeLoadFailures(failures)!;
    expect(msg).toContain("Could not load 5 files");
    expect(msg).toContain("f0.wav (bad); f1.wav (bad); f2.wav (bad)");
    expect(msg).not.toContain("f3.wav");
    expect(msg).toMatch(/and 2 more$/);
  });
});
