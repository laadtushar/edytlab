import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";
import test from "node:test";
import { fileURLToPath, pathToFileURL } from "node:url";

import { SHARED_SOURCES } from "./paths.mjs";

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

/** Give each actual CLI run its own social project and output link fixtures.
 * The space, # and ? in its path catch URL-pathname and raw file-URL mistakes. */
function fixture(t) {
  const scratch = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), "social artifact #paths?-")));
  t.after(() => fs.rmSync(scratch, { recursive: true, force: true }));
  const repo = path.join(scratch, "repo");
  const artifact = path.join(repo, "marketing/social-kit/artifact");
  fs.mkdirSync(artifact, { recursive: true });
  for (const name of ["build.mjs", "launch-hub.mjs", "paths.mjs"]) {
    fs.copyFileSync(path.join(HERE, name), path.join(artifact, name));
  }
  // Shared sources must be real files at their own paths, so the fixture holds
  // copies of those the builds read (the frame recordings are not read here).
  for (const entry of SHARED_SOURCES.filter((name) => !name.endsWith(".mp4"))) {
    fs.mkdirSync(path.dirname(path.join(repo, entry)), { recursive: true });
    fs.copyFileSync(path.join(REPO, entry), path.join(repo, entry));
  }
  const outside = path.join(scratch, "outside");
  fs.mkdirSync(outside);
  fs.writeFileSync(path.join(outside, "sentinel"), "untouched");
  return { scratch, repo, artifact, outside };
}

/** Execute the entrypoint with provider-free filesystem effect recording. */
function run(dirs, name, realDirectories = false) {
  const report = path.join(dirs.scratch, "effects.json");
  const result = spawnSync(process.execPath, ["--input-type=module", "-e", recorder,
    path.join(dirs.artifact, name), dirs.repo, REPO, report, realDirectories ? "real" : "stub"],
  { encoding: "utf8", maxBuffer: 4 * 1024 * 1024 });
  assert.ifError(result.error);
  return { ...result, effects: JSON.parse(fs.readFileSync(report, "utf8")) };
}

/** A refusal must precede even cleanup or mkdir and identify the project root. */
function refused(dirs, name, destination) {
  const result = run(dirs, name);
  assert.equal(result.status, 1, result.stderr);
  assert.match(result.stderr, /Refused path/);
  const end = result.stderr.indexOf(": must resolve inside ");
  const rejected = JSON.parse(result.stderr.slice("Refused path ".length, end));
  assert.ok(rejected === destination || rejected.startsWith(destination + path.sep), result.stderr);
  assert.ok(result.stderr.includes(path.dirname(dirs.artifact)), result.stderr);
  assert.deepEqual(result.effects, []);
  assert.equal(fs.readFileSync(path.join(dirs.outside, "sentinel"), "utf8"), "untouched");
  assert.ok(fs.lstatSync(destination).isSymbolicLink());
}

test("reviewed copy and launch assets still build through the actual entrypoints", (t) => {
  const dirs = fixture(t);
  const copy = run(dirs, "build.mjs");
  assert.equal(copy.status, 0, copy.stderr);
  assert.equal(copy.effects.length, 1);
  const html = Buffer.from(copy.effects[0].base64, "base64");
  assert.match(html.toString(), /LinkedIn/);
  assert.equal(copy.stdout, `wrote social-kit.html ${html.length} bytes\n`);

  const hub = run(dirs, "launch-hub.mjs");
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
    const dirs = fixture(t);
    const destination = path.join(dirs.artifact, "social-kit.html");
    fs.symlinkSync(path.join(dirs.outside, dangling ? "missing" : "sentinel"), destination);
    refused(dirs, "build.mjs", destination);
  });
}

for (const relative of ["out", "out/launch-hub", "out/launch-hub/index.html",
  "out/launch-hub/files.json", "out/launch-hub/existing-files.txt",
  "out/launch-hub/checklist-ids.txt", "out/launch-hub/linkedin",
  "out/launch-hub/linkedin/post-1-launch.png", "out/launch-hub/posters",
  "out/launch-hub/posters/launch-video.jpg"]) {
  for (const dangling of [false, true]) {
    test(`launch preflights ${relative} with an ${dangling ? "dangling" : "existing"} escape`, (t) => {
      const dirs = fixture(t);
      const destination = path.join(dirs.artifact, relative);
      fs.mkdirSync(path.dirname(destination), { recursive: true });
      fs.symlinkSync(path.join(dirs.outside, dangling ? "missing" : ""), destination);
      refused(dirs, "launch-hub.mjs", destination);
    });
  }
}

test("an in-project OUT link keeps lexical cleanup semantics and its target intact", (t) => {
  const dirs = fixture(t);
  const target = path.join(dirs.artifact, "retained-target");
  fs.mkdirSync(target);
  fs.writeFileSync(path.join(target, "sentinel"), "retained");
  fs.mkdirSync(path.join(dirs.artifact, "out"));
  const out = path.join(dirs.artifact, "out/launch-hub");
  fs.symlinkSync(target, out);
  const result = run(dirs, "launch-hub.mjs", true);
  assert.equal(result.status, 0, result.stderr);
  assert.equal(result.effects[0].kind, "rm");
  assert.equal(result.effects[0].name, out);
  assert.ok(fs.lstatSync(out).isDirectory());
  assert.equal(fs.readFileSync(path.join(target, "sentinel"), "utf8"), "retained");
});

test("an old regular OUT file is still replaced with the output directory", (t) => {
  const dirs = fixture(t);
  const out = path.join(dirs.artifact, "out/launch-hub");
  fs.mkdirSync(path.dirname(out));
  fs.writeFileSync(out, "old output");
  const result = run(dirs, "launch-hub.mjs", true);
  assert.equal(result.status, 0, result.stderr);
  assert.equal(result.effects[0].kind, "rm");
  assert.equal(result.effects[0].name, out);
  assert.ok(fs.lstatSync(out).isDirectory());
});

for (const existing of [false, true]) {
  test(`copy preflights a link parent after following its ${existing ? "existing" : "dangling"} target`, (t) => {
    const dirs = fixture(t);
    fs.mkdirSync(path.join(dirs.outside, "nested"));
    fs.symlinkSync(path.join(dirs.outside, "nested"), path.join(dirs.artifact, "bridge"));
    if (existing) fs.writeFileSync(path.join(dirs.outside, "victim"), "untouched");
    const destination = path.join(dirs.artifact, "social-kit.html");
    // Keep the raw target: path.join would remove the parent component itself.
    fs.symlinkSync("bridge/../victim", destination);
    refused(dirs, "build.mjs", destination);
    assert.equal(fs.existsSync(path.join(dirs.outside, "victim")), existing);
    if (existing) assert.equal(fs.readFileSync(path.join(dirs.outside, "victim"), "utf8"), "untouched");
  });
}

test("copy accepts an internal link with a parent component without changing its lexical write", (t) => {
  const dirs = fixture(t);
  fs.mkdirSync(path.join(dirs.artifact, "retained", "nested"), { recursive: true });
  fs.symlinkSync(path.join(dirs.artifact, "retained", "nested"), path.join(dirs.artifact, "bridge"));
  const destination = path.join(dirs.artifact, "social-kit.html");
  fs.symlinkSync("bridge/../generated.html", destination);
  const result = run(dirs, "build.mjs");
  assert.equal(result.status, 0, result.stderr);
  assert.equal(result.effects.length, 1);
  assert.equal(result.effects[0].name, destination);
});

test("nested dangling links follow parent components before preflight", (t) => {
  const dirs = fixture(t);
  fs.mkdirSync(path.join(dirs.outside, "nested"));
  fs.symlinkSync(path.join(dirs.outside, "nested"), path.join(dirs.artifact, "bridge"));
  fs.symlinkSync("bridge/../victim", path.join(dirs.artifact, "intermediate"));
  const destination = path.join(dirs.artifact, "social-kit.html");
  fs.symlinkSync("intermediate", destination);
  refused(dirs, "build.mjs", destination);
  assert.equal(fs.existsSync(path.join(dirs.outside, "victim")), false);
});

test("cyclic output links fail before any output effect", (t) => {
  const dirs = fixture(t);
  const destination = path.join(dirs.artifact, "social-kit.html");
  fs.symlinkSync("social-kit.html", destination);
  const result = run(dirs, "build.mjs");
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Too many symbolic links/);
  assert.deepEqual(result.effects, []);
});

// ---------------------------------------------------------------- inputs

/** An input refusal must also precede every output effect and name the input. */
function refusedInput(dirs, name, source) {
  const result = run(dirs, name);
  assert.equal(result.status, 1, result.stderr);
  assert.match(result.stderr, /^Refused input /);
  assert.ok(result.stderr.includes(JSON.stringify(source)), result.stderr);
  assert.deepEqual(result.effects, []);
  assert.equal(fs.readFileSync(path.join(dirs.outside, "sentinel"), "utf8"), "untouched");
}

/** Load the fixture's own copy of paths.mjs, so its roots are the fixture's. */
function fixturePaths(dirs) {
  return import(`${pathToFileURL(path.join(dirs.artifact, "paths.mjs")).href}?${Math.random()}`);
}

test("the launch hub reads the allowlisted shared sources at their own paths", (t) => {
  const dirs = fixture(t);
  const pressKit = path.join(dirs.repo, "marketing/press-kit.md");
  fs.writeFileSync(pressKit, fs.readFileSync(pressKit, "utf8")
    .replace("AI what you want done.", "AI what you want done. Fixture marker 7f3a."));
  const hub = run(dirs, "launch-hub.mjs");
  assert.equal(hub.status, 0, hub.stderr);
  const html = Buffer.from(hub.effects.find((e) => e.name.endsWith("index.html")).base64, "base64");
  assert.match(html.toString(), /Fixture marker 7f3a/);
  const poster = hub.effects.find((e) => e.kind === "copy" && e.name.endsWith("posters/launch-video.jpg"));
  assert.equal(poster.from, path.join(dirs.repo, "website/public/videos/launch-video.jpg"));
});

for (const entry of ["marketing/press-kit.md", "website/lib/videos.ts",
  "website/public/videos/launch-video.jpg"]) {
  test(`the launch hub refuses ${entry} when a link sends it outside`, (t) => {
    const dirs = fixture(t);
    const source = path.join(dirs.repo, entry);
    fs.rmSync(source);
    fs.symlinkSync(path.join(dirs.outside, "sentinel"), source);
    refusedInput(dirs, "launch-hub.mjs", source);
  });
}

test("a shared source is refused when a link in its parent redirects it", (t) => {
  const dirs = fixture(t);
  const videos = path.join(dirs.repo, "website/public/videos");
  fs.cpSync(videos, path.join(dirs.outside, "videos"), { recursive: true });
  fs.rmSync(videos, { recursive: true });
  fs.symlinkSync(path.join(dirs.outside, "videos"), videos);
  refusedInput(dirs, "launch-hub.mjs", path.join(videos, "launch-video.jpg"));
});

test("a shared source is refused when it is redirected to another file in the repository", (t) => {
  const dirs = fixture(t);
  const source = path.join(dirs.repo, "website/lib/videos.ts");
  fs.renameSync(source, path.join(dirs.repo, "website/lib/moved.ts"));
  fs.symlinkSync("moved.ts", source);
  refusedInput(dirs, "launch-hub.mjs", source);
});

test("a project-local input linked outside the project is refused before output", (t) => {
  const dirs = fixture(t);
  const template = path.join(dirs.artifact, "template.html");
  fs.symlinkSync(path.join(dirs.outside, "sentinel"), template);
  refusedInput(dirs, "build.mjs", template);
  const copy = path.join(dirs.repo, "marketing/social-kit/calendar.md");
  fs.symlinkSync(path.join(dirs.outside, "sentinel"), copy);
  refusedInput(dirs, "launch-hub.mjs", copy);
});

test("only project paths and the listed shared sources pass the input check", async (t) => {
  const dirs = fixture(t);
  const { checkInputPath, SHARED_SOURCES } = await fixturePaths(dirs);
  const local = path.join(dirs.repo, "marketing/social-kit/linkedin-posts.md");
  assert.equal(checkInputPath(local), local);
  for (const entry of SHARED_SOURCES.filter((name) => !name.endsWith(".mp4"))) {
    assert.equal(checkInputPath(path.join(dirs.repo, entry)), path.join(dirs.repo, entry));
  }
  // Not listed, though inside the repository or a listed folder, or outside it entirely.
  for (const name of ["README.md", "marketing/launch/README.md", "website/lib/other.ts",
    "website/public/videos/other.jpg", "../outside/sentinel"]) {
    const source = path.join(dirs.repo, name);
    fs.mkdirSync(path.dirname(source), { recursive: true });
    if (!fs.existsSync(source)) fs.writeFileSync(source, "not listed");
    assert.throws(() => checkInputPath(source), /^Error: Refused input /, name);
  }
  // A listed name must be a regular file.
  const missing = path.join(dirs.repo, "marketing/press-kit.md");
  fs.rmSync(missing);
  assert.throws(() => checkInputPath(missing), /^Error: Refused input /);
  fs.mkdirSync(missing);
  assert.throws(() => checkInputPath(missing), /^Error: Refused input /);
});

/** A fixture install of the allowlisted font package in pnpm's layout. */
function fontPackage(dirs, store = path.join(dirs.repo, "node_modules/.pnpm/pkg/node_modules/@fontsource-variable/geist-mono")) {
  fs.mkdirSync(path.join(store, "files"), { recursive: true });
  fs.writeFileSync(path.join(store, "package.json"), JSON.stringify({ name: "@fontsource-variable/geist-mono", version: "0.0.0" }));
  fs.writeFileSync(path.join(store, "files/geist-mono-latin-wght-normal.woff2"), "font");
  const desktop = path.join(dirs.repo, "apps/desktop");
  fs.mkdirSync(path.join(desktop, "node_modules/@fontsource-variable"), { recursive: true });
  if (!fs.existsSync(path.join(desktop, "package.json"))) fs.writeFileSync(path.join(desktop, "package.json"), "{}");
  fs.symlinkSync(store, path.join(desktop, "node_modules/@fontsource-variable/geist-mono"));
  return store;
}

test("the allowlisted package font resolves through pnpm's link to the installed file", async (t) => {
  const dirs = fixture(t);
  const store = fontPackage(dirs);
  const { packageSourcePath } = await fixturePaths(dirs);
  const font = "files/geist-mono-latin-wght-normal.woff2";
  assert.equal(packageSourcePath("@fontsource-variable/geist-mono", font), path.join(store, font));
  for (const [specifier, file] of [["@fontsource-variable/geist-mono", "package.json"],
    ["@fontsource-variable/geist-mono", "files/other.woff2"], ["@fontsource-variable/geist", font]]) {
    assert.throws(() => packageSourcePath(specifier, file), /not an allowlisted package source/);
  }
});

test("a package font installed outside the checkout is refused", async (t) => {
  const dirs = fixture(t);
  fontPackage(dirs, path.join(dirs.outside, "node_modules/@fontsource-variable/geist-mono"));
  const { packageSourcePath } = await fixturePaths(dirs);
  assert.throws(() => packageSourcePath("@fontsource-variable/geist-mono",
    "files/geist-mono-latin-wght-normal.woff2"), /^Error: Refused input /);
});

test("a package font file linked outside the checkout is refused", async (t) => {
  const dirs = fixture(t);
  const store = fontPackage(dirs);
  const font = path.join(store, "files/geist-mono-latin-wght-normal.woff2");
  fs.rmSync(font);
  fs.symlinkSync(path.join(dirs.outside, "sentinel"), font);
  const { packageSourcePath } = await fixturePaths(dirs);
  assert.throws(() => packageSourcePath("@fontsource-variable/geist-mono",
    "files/geist-mono-latin-wght-normal.woff2"), /^Error: Refused input /);
});

/** extract-frames.sh in the fixture, with an ffmpeg that only records its calls.
 * prepare, when given, changes the fixture's images folder before the script runs. */
function frames(dirs, prepare) {
  const images = path.join(dirs.repo, "marketing/social-kit/images");
  fs.mkdirSync(images, { recursive: true });
  fs.copyFileSync(path.join(HERE, "../images/extract-frames.sh"), path.join(images, "extract-frames.sh"));
  for (const entry of SHARED_SOURCES.filter((name) => name.endsWith(".mp4"))) {
    fs.mkdirSync(path.dirname(path.join(dirs.repo, entry)), { recursive: true });
    fs.writeFileSync(path.join(dirs.repo, entry), "recording");
  }
  if (prepare) prepare(images);
  const bin = path.join(dirs.scratch, "bin");
  fs.mkdirSync(bin);
  const log = path.join(dirs.scratch, "ffmpeg.log");
  fs.writeFileSync(path.join(bin, "ffmpeg"), `#!/bin/sh\necho "$@" >> '${log}'\n`, { mode: 0o755 });
  const result = spawnSync("bash", [path.join(images, "extract-frames.sh")],
    { encoding: "utf8", env: { ...process.env, PATH: `${bin}${path.delimiter}${process.env.PATH}` } });
  assert.ifError(result.error);
  const calls = fs.existsSync(log) ? fs.readFileSync(log, "utf8").trim().split("\n") : [];
  return { ...result, calls, out: path.join(images, "frames") };
}

test("extract-frames reads the allowlisted recordings into the project", { skip: process.platform === "win32" }, (t) => {
  const dirs = fixture(t);
  const result = frames(dirs);
  assert.equal(result.status, 0, result.stderr);
  assert.equal(result.calls.length, 12);
  assert.ok(result.calls.every((call) => / -i (marketing\/promo-v0\.7\.0\/out|website\/public\/demos)\//.test(call)));
  assert.ok(fs.statSync(result.out).isDirectory());
});

for (const [label, prepare, refusal] of [
  ["a recording linked outside", (dirs) => {
    const demo = path.join(dirs.repo, "website/public/demos/dj-mini-mix.mp4");
    fs.rmSync(demo);
    fs.symlinkSync(path.join(dirs.outside, "sentinel"), demo);
  }, /^Refused input "website\/public\/demos\/dj-mini-mix\.mp4"/],
  ["a frames folder linked outside", (dirs, images) => {
    fs.symlinkSync(dirs.outside, path.join(images, "frames"));
  }, /^Refused path "marketing\/social-kit\/images\/frames"/],
]) {
  test(`extract-frames refuses ${label} before writing a frame`, { skip: process.platform === "win32" }, (t) => {
    const dirs = fixture(t);
    const result = frames(dirs, (images) => prepare(dirs, images));
    assert.equal(result.status, 1, result.stderr);
    assert.match(result.stderr, refusal);
    assert.deepEqual(result.calls, []);
    assert.deepEqual(fs.readdirSync(dirs.outside), ["sentinel"]);
  });
}

// ---------------------------------------------------------------- video lists

test("the video lists accept no space, a tab or several spaces after a field's colon", (t) => {
  const dirs = fixture(t);
  const page = () => {
    const hub = run(dirs, "launch-hub.mjs");
    assert.equal(hub.status, 0, hub.stderr);
    return Buffer.from(hub.effects.find((e) => e.name.endsWith("index.html")).base64, "base64").toString();
  };
  const expected = page();
  const lists = ["website/lib/videos.ts", "website/lib/demos.ts"].map((name) => path.join(dirs.repo, name));
  const original = lists.map((file) => fs.readFileSync(file, "utf8"));
  for (const spacing of ["", "\t", "   ", " \t "]) {
    lists.forEach((file, i) => fs.writeFileSync(file,
      original[i].replace(/^(\s*(?:title|seconds|width|height)): /gm, `$1:${spacing}`)));
    assert.notEqual(fs.readFileSync(lists[0], "utf8"), original[0]);
    assert.equal(page(), expected, JSON.stringify(spacing));
  }
});

// ---------------------------------------------------------------- render outputs

/** A social project holding the renders and their modules, with nothing installed. */
function renders(t) {
  const dirs = fixture(t);
  const kit = path.join(dirs.repo, "marketing/social-kit");
  for (const name of ["images/render.mjs", "images/themes.mjs", "linkedin/render.mjs",
    "linkedin/lint.mjs", "linkedin/tokens.mjs"]) {
    fs.mkdirSync(path.dirname(path.join(kit, name)), { recursive: true });
    fs.copyFileSync(path.join(HERE, "..", name), path.join(kit, name));
  }
  return { ...dirs, kit };
}

/** Every path below a directory, so a refused run can be shown to write nothing. */
function tree(directory) {
  return fs.readdirSync(directory, { recursive: true }).sort();
}

/** Run a render, which must refuse a destination before reading or writing anything. */
function renderRefused(dirs, script, args, cwd = path.dirname(path.join(dirs.kit, script))) {
  const before = [tree(dirs.repo), tree(dirs.outside)];
  const result = spawnSync(process.execPath, [path.join(dirs.kit, script), ...args], { cwd, encoding: "utf8" });
  assert.ifError(result.error);
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /Refused path .*: must resolve inside /);
  assert.deepEqual([tree(dirs.repo), tree(dirs.outside)], before);
  assert.equal(fs.readFileSync(path.join(dirs.outside, "sentinel"), "utf8"), "untouched");
}

test("images refuse --sheets outside the project", (t) => {
  const dirs = renders(t);
  renderRefused(dirs, "images/render.mjs", ["--sheets", "../../.."]);
});

test("images refuse --sheets through a symlinked directory", (t) => {
  const dirs = renders(t);
  fs.symlinkSync(dirs.outside, path.join(dirs.kit, "images/sheets"));
  renderRefused(dirs, "images/render.mjs", ["--sheets", "sheets"]);
});

for (const [script, folder] of [["images/render.mjs", "out"], ["images/render.mjs", ".build"],
  ["linkedin/render.mjs", "out"], ["linkedin/render.mjs", ".build"]]) {
  test(`${script} refuses a ${folder} folder linked outside`, (t) => {
    const dirs = renders(t);
    fs.symlinkSync(dirs.outside, path.join(path.dirname(path.join(dirs.kit, script)), folder));
    renderRefused(dirs, script, []);
  });
}
