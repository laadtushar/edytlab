#!/usr/bin/env node
// Checks (or rewrites) apps/desktop/src-tauri/src/pre_manifest_skills.json:
// the SHA-256 of every version of every skill under
// apps/desktop/src-tauri/resources/skills in the git history up to
// BOUNDARY, the last commit that changed them before the app kept a
// manifest of what it installed (#574).
//
// That file is FROZEN. The app uses it for one thing: an install with no
// `.bundled.json` can hold only versions shipped before the manifest did,
// so a file there counts as unedited if its text is one of these, and a
// skill listed here that is missing from the folder was deleted by the
// user. Adding a later version or a later skill would make a user who
// upgrades straight from such an install to a release that adds a skill
// take the new skill for one they deleted. So nothing needs to be added
// when a bundled skill changes, and the Rust test
// `the_pre_manifest_list_never_changes` fails if the file does.
//
// This script exists to show the file is what the history says:
//
//   node scripts/pre-manifest-skill-hashes.mjs --check   # exit 1 if it differs
//   node scripts/pre-manifest-skill-hashes.mjs           # rewrite it
//
// Needs the full history, so not a shallow clone (`git fetch --unshallow`).
//
// A hash is over the text with CRLF read as LF, to match the app's
// `bundled_skills::text_hash`: a Windows checkout writes CRLF, git's blob
// has LF, and they are the same skill.

import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

// The last commit that touched the skills before the manifest: #516.
const BOUNDARY = "67d709278222a54d20ffe1f4b54047c4d34bfa6c";
const SKILLS_DIR = "apps/desktop/src-tauri/resources/skills";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const OUT = join(root, "apps/desktop/src-tauri/src/pre_manifest_skills.json");

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
try {
  git("cat-file", "-e", BOUNDARY + "^{commit}");
} catch {
  console.error(`Commit ${BOUNDARY} is not in this clone. Fetch the full history.`);
  process.exit(1);
}

// name -> hashes, oldest first, each once.
const versions = new Map();
const add = (name, hash) => {
  const list = versions.get(name) ?? [];
  if (!list.includes(hash)) list.push(hash);
  versions.set(name, list);
};

// Every commit up to BOUNDARY that touched the folder, oldest first, and the
// folder's `.md` blobs as that commit had them. Blobs repeat across commits,
// so each is read once.
const hashByBlob = new Map();
const commits = gitText("log", "--reverse", "--format=%H", BOUNDARY, "--", SKILLS_DIR)
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

const skills = Object.fromEntries([...versions].sort(([a], [b]) => (a < b ? -1 : 1)));
const json =
  JSON.stringify(
    {
      note:
        "FROZEN: the bundled skills as shipped before the app kept .bundled.json (history up to " +
        BOUNDARY.slice(0, 8) +
        "). Never add to it; see bundled_skills.rs.",
      generated_by: "node scripts/pre-manifest-skill-hashes.mjs",
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
    console.error("pre_manifest_skills.json is not what the history up to " + BOUNDARY.slice(0, 8) + " says.");
    process.exit(1);
  }
  console.log("pre_manifest_skills.json matches the history.");
} else {
  writeFileSync(OUT, json);
  const n = Object.values(skills).reduce((sum, l) => sum + l.length, 0);
  console.log(`Wrote ${OUT}: ${Object.keys(skills).length} skills, ${n} versions.`);
}
