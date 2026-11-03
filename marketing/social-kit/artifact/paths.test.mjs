import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";
import test from "node:test";
import { fileURLToPath } from "node:url";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const REPO = path.resolve(HERE, "../../..");

// Run the actual entrypoint, retaining real path inspection while recording all
// output effects. Source reads use the repository's reviewed copy and assets.
const recorder = String.raw`
import fs from "node:fs";
import path from "node:path";
import { createHash } from "node:crypto";
import { pathToFileURL } from "node:url";
const [entry, fixture, repo, report, directories] = process.argv.slice(1);
const original = { ...fs };
const effects = [];
const contents = new Map();
const source = (name) => {
  if (typeof name !== "string" || !name.startsWith(fixture + path.sep)
      || original.existsSync(name)) return name;
  return path.join(repo, path.relative(fixture, name));
};
fs.readFileSync = (name, ...args) => original.readFileSync(source(name), ...args);
fs.existsSync = (name) => original.existsSync(source(name));
fs.statSync = (name, ...args) => contents.has(name)
  ? { size: contents.get(name).length } : original.statSync(source(name), ...args);
fs.writeFileSync = (name, data) => {
  const bytes = Buffer.from(data);
  contents.set(name, bytes);
  effects.push({ kind: "write", name, base64: bytes.toString("base64") });
};
fs.copyFileSync = (from, name) => effects.push({ kind: "copy", from, name,
  sha256: createHash("sha256").update(original.readFileSync(source(from))).digest("hex") });
fs.mkdirSync = (name, options) => {
  effects.push({ kind: "mkdir", name });
  if (directories === "real") original.mkdirSync(name, options);
};
fs.rmSync = (name, options) => {
  effects.push({ kind: "rm", name });
  if (directories === "real") original.rmSync(name, options);
};
try {
  await import(pathToFileURL(entry));
} catch (error) {
  console.error(error.message);
  process.exitCode = 1;
} finally {
  original.writeFileSync(report, JSON.stringify(effects));
}
`;

/** Give each actual CLI run its own social project and output link fixtures. */
function fixture(t) {
  const scratch = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), "social-artifact-paths-")));
  t.after(() => fs.rmSync(scratch, { recursive: true, force: true }));
  const repo = path.join(scratch, "repo");
  const artifact = path.join(repo, "marketing/social-kit/artifact");
  fs.mkdirSync(artifact, { recursive: true });
  for (const name of ["build.mjs", "launch-hub.mjs", "paths.mjs"]) {
    fs.copyFileSync(path.join(HERE, name), path.join(artifact, name));
  }
  const outside = path.join(scratch, "outside");
  fs.mkdirSync(outside);
  fs.writeFileSync(path.join(outside, "sentinel"), "untouched");
  return { scratch, repo, artifact, outside };
}

/** Execute the entrypoint with provider-free filesystem effect recording. */
function run(f, name, realDirectories = false) {
  const report = path.join(f.scratch, "effects.json");
  const result = spawnSync(process.execPath, ["--input-type=module", "-e", recorder,
    path.join(f.artifact, name), f.repo, REPO, report, realDirectories ? "real" : "stub"],
  { encoding: "utf8", maxBuffer: 4 * 1024 * 1024 });
  assert.ifError(result.error);
  return { ...result, effects: JSON.parse(fs.readFileSync(report, "utf8")) };
}

/** A refusal must precede even cleanup or mkdir and identify the project root. */
function refused(f, name, destination) {
  const result = run(f, name);
  assert.equal(result.status, 1, result.stderr);
  assert.match(result.stderr, /Refused path/);
  const end = result.stderr.indexOf(": must resolve inside ");
  const rejected = JSON.parse(result.stderr.slice("Refused path ".length, end));
  assert.ok(rejected === destination || rejected.startsWith(destination + path.sep), result.stderr);
  assert.ok(result.stderr.includes(path.dirname(f.artifact)), result.stderr);
  assert.deepEqual(result.effects, []);
  assert.equal(fs.readFileSync(path.join(f.outside, "sentinel"), "utf8"), "untouched");
  assert.ok(fs.lstatSync(destination).isSymbolicLink());
}

test("reviewed copy and launch assets still build through the actual entrypoints", (t) => {
  const f = fixture(t);
  const copy = run(f, "build.mjs");
  assert.equal(copy.status, 0, copy.stderr);
  assert.equal(copy.effects.length, 1);
  const html = Buffer.from(copy.effects[0].base64, "base64");
  assert.match(html.toString(), /LinkedIn/);
  assert.equal(copy.stdout, `wrote social-kit.html ${html.length} bytes\n`);

  const hub = run(f, "launch-hub.mjs");
  assert.equal(hub.status, 0, hub.stderr);
  const writes = hub.effects.filter((effect) => effect.kind === "write");
  assert.deepEqual(writes.map((effect) => path.basename(effect.name)),
    ["index.html", "files.json", "existing-files.txt", "checklist-ids.txt"]);
  const map = JSON.parse(Buffer.from(writes[1].base64, "base64"));
  const copies = hub.effects.filter((effect) => effect.kind === "copy");
  assert.ok(copies.length > 6);
  assert.deepEqual(Object.values(map).sort(), copies.map((effect) => effect.name).sort());
  const entry = path.join("marketing", "social-kit", "artifact", "out", "launch-hub", "index.html");
  assert.ok(hub.stdout.startsWith(`wrote ${entry}:`), hub.stdout);
});

for (const dangling of [false, true]) {
  test(`copy artifact refuses an ${dangling ? "dangling" : "existing"} external HTML link`, (t) => {
    const f = fixture(t);
    const destination = path.join(f.artifact, "social-kit.html");
    fs.symlinkSync(path.join(f.outside, dangling ? "missing" : "sentinel"), destination);
    refused(f, "build.mjs", destination);
  });
}

for (const relative of ["out", "out/launch-hub", "out/launch-hub/index.html",
  "out/launch-hub/files.json", "out/launch-hub/existing-files.txt",
  "out/launch-hub/checklist-ids.txt", "out/launch-hub/linkedin",
  "out/launch-hub/linkedin/post-1-launch.png", "out/launch-hub/posters",
  "out/launch-hub/posters/launch-video.jpg"]) {
  for (const dangling of [false, true]) {
    test(`launch preflights ${relative} with an ${dangling ? "dangling" : "existing"} escape`, (t) => {
      const f = fixture(t);
      const destination = path.join(f.artifact, relative);
      fs.mkdirSync(path.dirname(destination), { recursive: true });
      fs.symlinkSync(path.join(f.outside, dangling ? "missing" : ""), destination);
      refused(f, "launch-hub.mjs", destination);
    });
  }
}

test("an in-project OUT link keeps lexical cleanup semantics and its target intact", (t) => {
  const f = fixture(t);
  const target = path.join(f.artifact, "retained-target");
  fs.mkdirSync(target);
  fs.writeFileSync(path.join(target, "sentinel"), "retained");
  fs.mkdirSync(path.join(f.artifact, "out"));
  const out = path.join(f.artifact, "out/launch-hub");
  fs.symlinkSync(target, out);
  const result = run(f, "launch-hub.mjs", true);
  assert.equal(result.status, 0, result.stderr);
  assert.equal(result.effects[0].kind, "rm");
  assert.equal(result.effects[0].name, out);
  assert.ok(fs.lstatSync(out).isDirectory());
  assert.equal(fs.readFileSync(path.join(target, "sentinel"), "utf8"), "retained");
});

test("an old regular OUT file is still replaced with the output directory", (t) => {
  const f = fixture(t);
  const out = path.join(f.artifact, "out/launch-hub");
  fs.mkdirSync(path.dirname(out));
  fs.writeFileSync(out, "old output");
  const result = run(f, "launch-hub.mjs", true);
  assert.equal(result.status, 0, result.stderr);
  assert.equal(result.effects[0].kind, "rm");
  assert.equal(result.effects[0].name, out);
  assert.ok(fs.lstatSync(out).isDirectory());
});
