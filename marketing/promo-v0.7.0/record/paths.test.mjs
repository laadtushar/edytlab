import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import test from "node:test";
import { PROJECT_ROOT, projectPath } from "./paths.mjs";

const CUTTER = join(PROJECT_ROOT, "record/make-footage.mjs");

function fixture(t) {
  const inside = mkdtempSync(join(PROJECT_ROOT, ".path-test-"));
  const outside = mkdtempSync(join(tmpdir(), "promo-record-test-"));
  t.after(() => {
    rmSync(inside, { recursive: true, force: true });
    rmSync(outside, { recursive: true, force: true });
  });
  return { inside, outside };
}

function refused(path) {
  assert.throws(() => projectPath(path), (error) => {
    assert.match(error.message, /Refused path/);
    assert.ok(error.message.includes(JSON.stringify(path)));
    assert.ok(error.message.includes(PROJECT_ROOT));
    return true;
  });
}

function cut(out, dest) {
  return spawnSync(process.execPath, [CUTTER, "s2-faders"], {
    env: { ...process.env, OUT: out, DEST: dest }, encoding: "utf8",
  });
}

test("existing inputs, nested new outputs and internal symlinks resolve", (t) => {
  const { inside } = fixture(t);
  const input = join(inside, "recording.mp4");
  writeFileSync(input, "fixture");
  assert.equal(projectPath(input), input);
  assert.equal(projectPath(join(inside, "new", "output.mp4")), join(inside, "new", "output.mp4"));
  symlinkSync(inside, join(inside, "alias"));
  assert.equal(projectPath(join(inside, "alias", "recording.mp4")), input);
});

test("absolute paths, traversal, escaped and dangling symlinks are refused", (t) => {
  const { inside, outside } = fixture(t);
  refused(join(outside, "new.mp4"));
  refused(join(PROJECT_ROOT, "..", "outside.mp4"));
  symlinkSync(outside, join(inside, "escaped"));
  refused(join(inside, "escaped", "new", "video.mp4"));
  symlinkSync(join(outside, "missing"), join(inside, "dangling"));
  refused(join(inside, "dangling", "new", "video.mp4"));
});

test("external recording runs and output folders fail before reading data", (t) => {
  const { inside, outside } = fixture(t);
  for (const [out, dest] of [[outside, inside], [inside, outside]]) {
    const result = cut(out, dest);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /Refused path/);
    assert.ok(result.stderr.includes(PROJECT_ROOT));
    assert.ok(result.stderr.includes(outside));
  }
});

test("metadata traversal and escaped input links fail before ffprobe", (t) => {
  const { inside, outside } = fixture(t);
  const recording = join(inside, "recording.mp4");
  symlinkSync(join(outside, "missing.mp4"), recording);
  for (const video of ["../../../escaped.mp4", "recording.mp4"]) {
    writeFileSync(join(inside, "results.json"), JSON.stringify([{ id: "9-promo-s2-faders-stay-put", status: "pass", video }]));
    const result = cut(inside, join(inside, "footage"));
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /Refused path/);
    assert.doesNotMatch(result.stderr, /Command failed/);
  }
});

test("symlinked recording metadata cannot be read", (t) => {
  const { inside, outside } = fixture(t);
  const external = join(outside, "results.json");
  writeFileSync(external, "[]");
  symlinkSync(external, join(inside, "results.json"));
  const result = cut(inside, join(inside, "footage"));
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /Refused path/);
  assert.equal(readFileSync(external, "utf8"), "[]");
});

test("help requires no native-suite output folder", () => {
  const result = spawnSync(process.execPath, [CUTTER, "--help"], { env: {}, encoding: "utf8" });
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /Copy external native runs/);
});
