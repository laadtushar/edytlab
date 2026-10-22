/**
 * Only the user's click downloads a model (#383).
 *
 * A model is about 139 MB and the requests go to huggingface.co. The rules
 * are that no tool, agent path or launch code starts one: the one call in
 * the app is the Download (and Retry) button of the Settings row. This
 * reads the authored source and fails when:
 *
 * - the command's name, `download_ml_model`, appears in `src/` anywhere
 *   but `lib/tauri-bridge.ts`, so no other file can `invoke` it;
 * - `downloadMlModel` is used in any file but the bridge (which defines it)
 *   and the row;
 * - in the row, `downloadMlModel` is used anywhere but its import and one
 *   call inside the `download` handler; or
 * - in the row, the `download` handler is used for anything but a button's
 *   `onClick={download}`. It must not be handed to `useEffect`, called from
 *   an effect, or called at all: each of those would download without a
 *   click, for instance when Settings opens.
 *
 * The row is read as a syntax tree, not as text, so a word in a string or in
 * JSX text (`"Resume download"`, `data-testid="speech-model-download"`) is
 * not a use of the handler, and `useEffect(download, [])` or
 * `download?.()` is one however it is spelt.
 *
 * The backend half of the same rule is
 * `crates/ml-pipeline/tests/only_the_user_downloads.rs`.
 */

import { readdirSync, readFileSync } from "node:fs";
import { extname, join, relative } from "node:path";
import ts from "typescript";
import { describe, expect, it } from "vitest";

const SRC = join(process.cwd(), "src");
const ROW = join(SRC, "components/SpeechModelRow.tsx");
const BRIDGE = "lib/tauri-bridge.ts";

function sources(dir: string = SRC): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) {
      return entry.name === "__tests__" || entry.name === "node_modules" ? [] : sources(path);
    }
    if (![".ts", ".tsx"].includes(extname(path))) return [];
    if (/\.test\.tsx?$/.test(path)) return [];
    return [path];
  });
}

const rel = (path: string) => relative(SRC, path).split("\\").join("/");

/** Every identifier called `name` in the syntax tree of the file at `path`. */
function identifiersIn(path: string, name: string): ts.Identifier[] {
  const file = ts.createSourceFile(
    path,
    readFileSync(path, "utf8"),
    ts.ScriptTarget.Latest,
    true,
    ts.ScriptKind.TSX,
  );
  const found: ts.Identifier[] = [];
  const visit = (node: ts.Node) => {
    if (ts.isIdentifier(node) && node.text === name) found.push(node);
    ts.forEachChild(node, visit);
  };
  visit(file);
  return found;
}

/** `const <name> = …`: where the identifier is the declared name. */
const isDeclarationOf = (id: ts.Identifier) =>
  ts.isVariableDeclaration(id.parent) && id.parent.name === id;

/** `onClick={<id>}`: the identifier alone, as a JSX attribute's value. */
const isOnClickValue = (id: ts.Identifier) =>
  ts.isJsxExpression(id.parent) &&
  id.parent.expression === id &&
  ts.isJsxAttribute(id.parent.parent) &&
  id.parent.parent.name.getText() === "onClick";

/** Whether `id` sits inside `const download = …`. */
function isInsideDownloadHandler(id: ts.Identifier): boolean {
  for (let node: ts.Node | undefined = id.parent; node; node = node.parent) {
    if (
      ts.isVariableDeclaration(node) &&
      ts.isIdentifier(node.name) &&
      node.name.text === "download"
    ) {
      return true;
    }
  }
  return false;
}

describe("downloading a model", () => {
  it("names the command in the bridge and nowhere else in src/", () => {
    const files = sources()
      .filter((p) => readFileSync(p, "utf8").includes("download_ml_model"))
      .map(rel);
    expect(files).toEqual([BRIDGE]);
  });

  it("uses downloadMlModel only in the bridge and the Settings row", () => {
    const callers = sources()
      .filter((p) => identifiersIn(p, "downloadMlModel").length > 0)
      .map(rel)
      .sort();
    expect(callers).toEqual(["components/SpeechModelRow.tsx", BRIDGE]);
  });

  it("calls downloadMlModel once in the row, inside the download handler", () => {
    const uses = identifiersIn(ROW, "downloadMlModel");
    const imports = uses.filter((id) => ts.isImportSpecifier(id.parent));
    const rest = uses.filter((id) => !ts.isImportSpecifier(id.parent));

    expect(imports).toHaveLength(1);
    expect(rest).toHaveLength(1);
    expect(ts.isCallExpression(rest[0]!.parent)).toBe(true);
    expect(isInsideDownloadHandler(rest[0]!)).toBe(true);
  });

  it("uses the download handler only as a button's onClick", () => {
    const uses = identifiersIn(ROW, "download");
    const defined = uses.filter(isDeclarationOf);
    const clicks = uses.filter(isOnClickValue);
    const others = uses.filter((id) => !isDeclarationOf(id) && !isOnClickValue(id));

    // Where the handler is used for anything else, the line is named.
    expect(
      others.map((id) => id.getSourceFile().getLineAndCharacterOfPosition(id.getStart()).line + 1),
      "`download` is used for something other than `onClick={download}`",
    ).toEqual([]);
    expect(defined).toHaveLength(1);
    // Download (or Resume), and Retry.
    expect(clicks).toHaveLength(2);
  });
});
