#!/usr/bin/env node
// Regenerates apps/desktop/src-tauri/src/bundled_skills_shipped.json: the
// SHA-256 of every version of every skill under
// apps/desktop/src-tauri/resources/skills that the repository has ever had,
// plus the files as they are in the working tree now.
//
// Why: the app copies those skills into the user's ~/.edytlab/skills and, at
// each launch, replaces a copy with the newer bundled text only when the user
// has not edited it (#574). An install from before the app kept a record
// (`.bundled.json`) has no record to tell it, so such a file counts as
// unedited only when its text is one of these. Change a bundled skill and the
// Rust test `every_bundled_skill_is_in_the_shipped_list` fails until this has
// been run and the result committed.
//
//   node scripts/bundled-skill-hashes.mjs           # rewrite the file
//   node scripts/bundled-skill-hashes.mjs --check   # exit 1 if it is stale
//
// Needs the full history, so not a shallow clone (`git fetch --unshallow`):
// from a truncated history it would drop versions that did ship.
//
// A hash is over the text with CRLF read as LF, to match the app's
// `bundled_skills::text_hash`: a Windows checkout writes CRLF, git's blob
// has LF, and they are the same skill.

import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { readdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const SKILLS_DIR = "apps/desktop/src-tauri/resources/skills";
const OUT = join(root, "apps/desktop/src-tauri/src/bundled_skills_shipped.json");

const git = (...args) =>
  execFileSync("git", args, { cwd: root, maxBuffer: 256 * 1024 * 1024 });
const gitText = (...args) => git(...args).toString("utf8");

function hashOf(buf) {
  const lf = Buffer.from(buf.toString("latin1").replace(/\r\n/g, "\n"), "latin1");
  return createHash("sha256").update(lf).digest("hex");
}

if (gitText("rev-parse", "--is-shallow-repository").trim() === "true") {
  console.error(
    "This is a shallow clone, so the history of " + SKILLS_DIR + " is cut short.\n" +
      "Run `git fetch --unshallow` and try again.",
  );
  process.exit(1);
}

// name -> hashes, oldest first, each once.
const versions = new Map();
const add = (name, hash) => {
  const list = versions.get(name) ?? [];
  if (!list.includes(hash)) list.push(hash);
  versions.set(name, list);
};

// Every commit that touched the folder, oldest first, and the folder's
// `.md` blobs as that commit had them. Blobs repeat across commits, so each
// is read once.
const hashByBlob = new Map();
const commits = gitText("log", "--reverse", "--format=%H", "--", SKILLS_DIR)
  .split("\n")
  .filter(Boolean);
for (const commit of commits) {
  const listing = gitText("ls-tree", "-r", "-z", commit, "--", SKILLS_DIR);
  for (const row of listing.split("\0").filter(Boolean)) {
    // "<mode> blob <sha>\t<path>"
    const [meta, path] = row.split("\t");
    const blob = meta.split(" ")[2];
    if (!path.endsWith(".md")) continue;
    if (!hashByBlob.has(blob)) hashByBlob.set(blob, hashOf(git("cat-file", "blob", blob)));
    add(path.slice(path.lastIndexOf("/") + 1), hashByBlob.get(blob));
  }
}

// The files as they are now, committed or not: the edit being made.
for (const name of readdirSync(join(root, SKILLS_DIR)).sort()) {
  if (name.endsWith(".md")) add(name, hashOf(readFileSync(join(root, SKILLS_DIR, name))));
}

const skills = Object.fromEntries([...versions].sort(([a], [b]) => (a < b ? -1 : 1)));
const json =
  JSON.stringify(
    {
      generated_by: "node scripts/bundled-skill-hashes.mjs",
      hash: "SHA-256, lowercase hex, of the file's text with CRLF read as LF; oldest version first",
      skills,
    },
    null,
    2,
  ) + "\n";

if (process.argv.includes("--check")) {
  let current = "";
  try {
    current = readFileSync(OUT, "utf8");
  } catch {}
  if (current !== json) {
    console.error(
      "bundled_skills_shipped.json is out of date. Run `node scripts/bundled-skill-hashes.mjs` and commit it.",
    );
    process.exit(1);
  }
  console.log("bundled_skills_shipped.json is up to date.");
} else {
  writeFileSync(OUT, json);
  const n = Object.values(skills).reduce((sum, l) => sum + l.length, 0);
  console.log(`Wrote ${OUT}: ${Object.keys(skills).length} skills, ${n} versions.`);
}
