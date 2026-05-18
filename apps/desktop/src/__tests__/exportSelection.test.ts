import { describe, expect, it } from "vitest";

describe("export selection range validation", () => {
  it("start must be less than end", () => {
    const validate = (start: number, end: number) => start >= 0 && end > start;
    expect(validate(1.0, 5.0)).toBe(true);
    expect(validate(5.0, 1.0)).toBe(false);
    expect(validate(0, 0)).toBe(false);
  });

  it("full session export uses no range restriction", () => {
    const isFullExport = (start?: number, end?: number) =>
      start === undefined && end === undefined;
    expect(isFullExport()).toBe(true);
    expect(isFullExport(0, 10)).toBe(false);
  });
});
