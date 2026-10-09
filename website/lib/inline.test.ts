import { describe, expect, it } from "vitest";

import { isExternal, linksIn, parseInline, stripInline } from "./inline";

describe("parseInline", () => {
  it("passes plain text through as one token", () => {
    expect(parseInline("just words, 1 + 1 = 2")).toEqual([
      { type: "text", text: "just words, 1 + 1 = 2" },
    ]);
  });

  it("reads links, code and bold in order", () => {
    expect(parseInline("See [the docs](/docs), run `load x.wav`, **now**.")).toEqual([
      { type: "text", text: "See " },
      { type: "link", text: "the docs", href: "/docs" },
      { type: "text", text: ", run " },
      { type: "code", text: "load x.wav" },
      { type: "text", text: ", " },
      { type: "bold", text: "now" },
      { type: "text", text: "." },
    ]);
  });

  it("leaves brackets that are not links alone", () => {
    expect(stripInline("a [b] c (d)")).toBe("a [b] c (d)");
  });
});

describe("stripInline / linksIn / isExternal", () => {
  it("drops the markup and keeps the words", () => {
    expect(stripInline("[a](/x) and `b` and **c**")).toBe("a and b and c");
  });

  it("lists the hrefs", () => {
    expect(linksIn("[a](/x) [b](https://e.com/y)")).toEqual(["/x", "https://e.com/y"]);
  });

  it("calls only off-site hrefs external", () => {
    expect(isExternal("https://github.com/x")).toBe(true);
    expect(isExternal("//cdn.example.com/x")).toBe(true);
    expect(isExternal("/docs")).toBe(false);
    expect(isExternal("/#demos")).toBe(false);
  });
});
