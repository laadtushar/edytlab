import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import test from "node:test";
import { fileURLToPath, pathToFileURL } from "node:url";
import { runInNewContext } from "node:vm";

const moduleUrl = new URL("./tokens.mjs", import.meta.url);
const source = readFileSync(moduleUrl, "utf8");
const sectionEnd = source.indexOf("/** One property's value");
const privateSource = source.slice(source.indexOf("function declarationNameChar("), sectionEnd);
const scan = runInNewContext(`${privateSource}\ndeclarations;`);
const originalSource = String.raw`function declarations(body) {
  return [...body.matchAll(/(--[\w-]+)\s*:\s*([^;]+);/g)].map((m) => [m[1], m[2].trim()]);
}`;
const original = runInNewContext(`${originalSource}\ndeclarations;`);

/** Compare plain data across VM realms without changing string contents. */
function plain(value) {
  return JSON.parse(JSON.stringify(value));
}

/** Keep the pre-change matcher as an independent compatibility oracle. */
function compare(body) {
  assert.ok(body.length <= 160, "the original backtracking oracle only receives bounded fixtures");
  assert.deepEqual(plain(scan(body)), plain(original(body)), JSON.stringify(body));
}

test("declaration names, malformed headers and duplicate order match the original", () => {
  const fixtures = [
    "", ";", "--x:;", "--x: ;", "--x:\n;", "--x: value", "--x:one;--x:two;",
    "--:v;---:v;----:v;", "prefix--x:v;--x-_:v;--1:v;--_:v;",
    "--a --b: x;", "--a\t! --b:x;", "--a::x;", "--a:b:c;--b:d;",
    "/* --comment: value; */ --real: next;", "--a: --nested: value;",
    "--a:;--b:valid;", "--a ; --b:; --c: \t;", "--a\u0000--b:x;",
    "--é:v;--aé:v;é--b:v;", "--𐐀:v;𐐀--a:v;", "--\ud800:v;--a:\udfff;",
    "--a\u180e:x;--b\u200b:x;--c\u0085:x;", "--a\ufeff:\ufeff;",
  ];
  const whitespace = [
    "\t", "\n", "\v", "\f", "\r", " ", "\u00a0", "\u1680", "\u2000", "\u2001",
    "\u2002", "\u2003", "\u2004", "\u2005", "\u2006", "\u2007", "\u2008", "\u2009",
    "\u200a", "\u2028", "\u2029", "\u202f", "\u205f", "\u3000", "\ufeff",
  ];
  for (const space of whitespace) {
    fixtures.push(`--x${space}:${space}value${space};`, `--x:${space};`);
  }
  for (let count = 0; count < 40; count++) {
    fixtures.push(`${"-".repeat(count)}:v;`, `a${"-".repeat(count)}z: v;`);
  }
  for (const fixture of fixtures) compare(fixture);
  assert.deepEqual(plain(scan("/* --a: one; */ --a: ; --a:three;")), [
    ["--a", "one"], ["--a", ""], ["--a", "three"],
  ]);
});

test("bounded exhaustive and seeded malformed strings preserve matcher semantics", () => {
  const alphabet = ["-", "a", ":", ";", " "];
  let layer = [""];
  for (let length = 0; length <= 7; length++) {
    for (const body of layer) compare(body);
    if (length < 7) layer = layer.flatMap((prefix) => alphabet.map((char) => prefix + char));
  }
  const units = ["-", "--", "a", "Z", "0", "_", ":", ";", " ", "\n", "\ufeff",
    "\u2000", "é", "\u200b", "\ud800", "/*", "*/", "{", "}"];
  let state = 0x9408786;
  const next = () => {
    state = (Math.imul(state, 1664525) + 1013904223) >>> 0;
    return state;
  };
  for (let trial = 0; trial < 20000; trial++) {
    const length = next() % 80;
    let body = "";
    for (let i = 0; i < length; i++) body += units[next() % units.length];
    compare(body);
  }
});

test("actual website token stylesheet bytes match the original declaration matcher", () => {
  const here = dirname(fileURLToPath(moduleUrl));
  const repo = join(here, "../../..");
  const fontPackage = join(repo, "apps/desktop/node_modules",
    "@fontsource-variable/geist-mono/package.json");
  const context = {
    readFileSync, dirname, join, fileURLToPath, pathToFileURL,
    createRequire: (filename) => {
      assert.equal(filename, join(repo, "apps/desktop/package.json"));
      return { resolve: (specifier) => {
        assert.equal(specifier, "@fontsource-variable/geist-mono/package.json");
        return fontPackage;
      } };
    },
  };
  const executable = source.replace(/^import .*;\n/gm, "")
    .replace("import.meta.url", JSON.stringify(moduleUrl.href)).replace(/^export /gm, "");
  const declarationsStart = executable.indexOf("function declarations(body) {");
  const declarationsEnd = executable.indexOf("/** One property's value");
  const baseline = executable.slice(0, declarationsStart) + originalSource + "\n\n"
    + executable.slice(declarationsEnd);
  const current = runInNewContext(`${executable}\ntokensCss();`, { ...context });
  const previous = runInNewContext(`${baseline}\ntokensCss();`, { ...context });
  assert.equal(current, previous);
  assert.ok(Buffer.byteLength(current) > 1000);
});

test("large failed headers and unterminated values complete with bounded scans", () => {
  const bodies = [
    [`${"-".repeat(500000)};--valid: yes;`, [["--valid", "yes"]]],
    [`--a:${" ".repeat(500000)}`, []],
    [`--a:${" ".repeat(500000)};`, [["--a", ""]]],
    [`${"--x ".repeat(125000)};--valid: yes;`, [["--valid", "yes"]]],
    [`${"a ".repeat(250000)};--valid: yes;`, [["--valid", "yes"]]],
  ];
  for (const [body, expected] of bodies) {
    const actual = runInNewContext(`${privateSource}\ndeclarations(body);`,
      { body }, { timeout: 10000 });
    assert.deepEqual(plain(actual), expected);
  }
});
