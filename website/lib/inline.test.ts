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

// `parseInline` was one regular expression, which read to the end of the text
// from every opener that never closed. It is written out now; this is the
// expression it replaced, as the reference for what a string means.
const OLD_TOKEN = /\[([^\]]+)\]\(([^)\s]+)\)|`([^`]+)`|\*\*([^*]+)\*\*/g;
function parseInlineByRegex(source: string) {
  const out: unknown[] = [];
  let last = 0;
  for (const m of source.matchAll(OLD_TOKEN)) {
    const at = m.index ?? 0;
    if (at > last) out.push({ type: "text", text: source.slice(last, at) });
    if (m[1] !== undefined) out.push({ type: "link", text: m[1], href: m[2] });
    else if (m[3] !== undefined) out.push({ type: "code", text: m[3] });
    else out.push({ type: "bold", text: m[4] });
    last = at + m[0].length;
  }
  if (last < source.length) out.push({ type: "text", text: source.slice(last) });
  return out;
}

describe("parseInline against the expression it replaced", () => {
  it("reads every string the way the expression did", () => {
    const pieces = ["[", "]", "(", ")", "*", "**", "`", "a", "b", " ", "\n", "[a](", "](x)", "http://x/y"];
    let seed = 7;
    const next = () => (seed = (seed * 1103515245 + 12345) & 0x7fffffff) / 0x7fffffff;
    for (let n = 0; n < 40000; n++) {
      let s = "";
      for (let i = Math.floor(next() * 16); i > 0; i--) s += pieces[Math.floor(next() * pieces.length)];
      expect(parseInline(s), JSON.stringify(s)).toEqual(parseInlineByRegex(s));
    }
  });

  it("reads a long line of openers that never close in one pass", () => {
    const started = Date.now();
    for (const opener of ["[", "[a](", "`", "**", "[a]("]) {
      parseInline(opener.repeat(40000));
      parseInline("[a](b".repeat(20000));
    }
    expect(Date.now() - started).toBeLessThan(2000);
  });
});
