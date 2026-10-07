import { describe, expect, it } from "vitest";
import { isSessionValue } from "../lib/controlValue";

describe("isSessionValue", () => {
  it("is true when the slider shows what the session holds", () => {
    expect(isSessionValue(-17, -17)).toBe(true);
    expect(isSessionValue(0, 0)).toBe(true);
  });

  it("is true across f32 rounding, which is how the session stores a pan", () => {
    expect(isSessionValue(Math.fround(0.02), 0.02)).toBe(true);
    expect(isSessionValue(Math.fround(-0.1), -0.1)).toBe(true);
  });

  it("is false for any real step, however small", () => {
    expect(isSessionValue(-17, -16.5)).toBe(false);
    expect(isSessionValue(0, 0.02)).toBe(false);
  });

  it("is false when the track is not in the session's list, so the edit is made", () => {
    expect(isSessionValue(undefined, 0)).toBe(false);
  });
});
