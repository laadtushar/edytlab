#!/usr/bin/env node
// Writes (or, with --check, verifies) the third-party notices file the
// installers carry and Settings shows:
//
//   apps/desktop/src-tauri/licenses/THIRD_PARTY_NOTICES.txt
//
// It is made of
//   - preface.txt        what the file is, and where the other notices are
//   - credits.txt        code the Rust crates were derived from, models, data
//   - the JavaScript packages that ended up in the production bundle, from
//     `vite build` with rollup-plugin-license (EDYTLAB_NOTICES_JSON), and the
//     font and CSS packages the bundle pulls in without a JS import
//   - the Rust crates of the desktop app for every platform, from cargo-about
//     (about.toml, about.hbs), ending with the MPL-2.0 source availability.
//
//   node scripts/third-party-notices/generate.mjs           # write the file
//   node scripts/third-party-notices/generate.mjs --check   # fail if it is stale
//
// Needs: cargo-about (CARGO_ABOUT_BIN, default `cargo-about` on PATH) at the
// version below, `cargo` with the registry reachable, and `pnpm install`.
// See docs/development-guide.md ("Third-party licences").

import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, readdirSync, statSync, writeFileSync, existsSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { createRequire } from "node:module";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

/** The cargo-about release the output was made with. The CI workflow
 *  (.github/workflows/notices.yml, CARGO_ABOUT_VERSION) pins the same. */
export const CARGO_ABOUT_VERSION = "0.9.2";

const here = dirname(fileURLToPath(import.meta.url));
const repo = resolve(here, "..", "..");
const OUT = join(repo, "apps/desktop/src-tauri/licenses/THIRD_PARTY_NOTICES.txt");
const DESKTOP_MANIFEST = "apps/desktop/src-tauri/Cargo.toml";

/** Licences a package in the JavaScript bundle may be under. Anything else
 *  (a GPL, LGPL, MPL, EPL, an unknown or missing licence) stops the script:
 *  a new kind of obligation is the project owner's decision, not a lockfile
 *  bump's. MPL-2.0 is accepted for Rust (its source availability is written
 *  out) but is not expected in the bundle, so it is not here. */
const JS_ALLOWED = new Set([
  "MIT",
  "ISC",
  "BSD-2-Clause",
  "BSD-3-Clause",
  "Apache-2.0",
  "OFL-1.1",
  "0BSD",
  "CC0-1.0",
  "Unlicense",
  "Zlib",
]);

/** Packages whose files are in the bundle but not through a JavaScript
 *  import, so rollup-plugin-license cannot see them: the fonts (main.tsx
 *  imports their CSS, Vite copies the .woff2 files) and the CSS framework. */
const CSS_AND_FONT_PACKAGES = [
  "@fontsource-variable/geist",
  "@fontsource-variable/geist-mono",
  "tailwindcss",
];

const RULE = "=".repeat(80);
const THIN = "-".repeat(80);

/** Code-unit order, so the output does not depend on the machine's locale. */
const by = (a, b) => (a < b ? -1 : a > b ? 1 : 0);

function run(cmd, args, opts = {}) {
  return execFileSync(cmd, args, {
    cwd: repo,
    encoding: "utf8",
    stdio: ["ignore", "pipe", "inherit"],
    maxBuffer: 256 * 1024 * 1024,
    ...opts,
  });
}

/** Same bytes on every OS: LF, no trailing blanks, no tabs-of-the-day. */
function tidy(text) {
  return text
    .replace(/\r\n?/g, "\n")
    .split("\n")
    .map((l) => l.replace(/[ \t]+$/, ""))
    .join("\n")
    .replace(/\n{4,}/g, "\n\n\n")
    // Leading blank lines go, a first line's own indentation (a licence title
    // centred over its text) stays.
    .replace(/^\n+/, "")
    .replace(/\s+$/, "");
}

/** Package metadata can carry a person's e-mail address; the notices name
 *  the holder, not the inbox. */
function withoutEmails(text) {
  return text.replace(/\s*<[^<>\s]+@[^<>\s]+>/g, "");
}

/** The `targets` of about.toml: every platform an installer is built for. */
function readTargets() {
  const toml = readFileSync(join(here, "about.toml"), "utf8");
  const block = /^targets\s*=\s*\[([^\]]*)\]/m.exec(toml);
  if (!block) throw new Error("about.toml has no targets");
  return [...block[1].matchAll(/"([^"]+)"/g)].map((m) => m[1]);
}

/** "name version" of every registry crate linked into the desktop app on any
 *  of `targets`: normal dependencies only, no proc-macro crates (which run at
 *  build time and are not in the binary), as `cargo tree` resolves them. */
function shippedCrates(targets) {
  const ids = new Set();
  for (const target of targets) {
    const out = run("cargo", [
      "tree",
      "--locked",
      "--manifest-path",
      DESKTOP_MANIFEST,
      "--edges",
      "normal,no-proc-macro",
      "--target",
      target,
      "--prefix",
      "none",
    ]);
    for (const line of out.split("\n")) {
      const m = /^(\S+) v(\S+)(?: \((.*?)\))?(?: \(\*\))?$/.exec(line.trim());
      if (!m) continue;
      // A crate of this workspace is shown with its folder: not a third party.
      if (m[3] && (m[3].startsWith("/") || /^[A-Za-z]:[\\/]/.test(m[3]))) continue;
      ids.add(`${m[1]} ${m[2]}`);
    }
  }
  return ids;
}

function cargoAbout(tmp) {
  const bin = process.env.CARGO_ABOUT_BIN || "cargo-about";
  const version = run(bin, ["--version"]).trim();
  const wanted = process.env.CARGO_ABOUT_VERSION || CARGO_ABOUT_VERSION;
  if (version !== `cargo-about ${wanted}`) {
    throw new Error(
      `expected cargo-about ${wanted}, found "${version}". Install the pinned one: ` +
        `cargo install --locked cargo-about@${wanted} --features cli`,
    );
  }
  // The crates must be on disk: cargo-about reads their licence files, and
  // `--offline` keeps it from asking the network for anything else.
  run("cargo", ["fetch", "--locked", "--manifest-path", DESKTOP_MANIFEST], { stdio: ["ignore", "ignore", "inherit"] });
  const config = ["--config", "scripts/third-party-notices/about.toml"];
  const out = join(tmp, "rust.txt");
  run(bin, [
    "generate",
    "--offline",
    "--manifest-path",
    DESKTOP_MANIFEST,
    ...config,
    "scripts/third-party-notices/about.hbs",
    "--output-file",
    out,
  ]);
  let text = readFileSync(out, "utf8");
  // The one line per crate that names who holds the copyright of a crate with
  // no licence file: the holder, never the e-mail address.
  text = text
    .split("\n")
    .map((l) => (l.includes("(no licence file in the crate; copyright:") ? withoutEmails(l) : l))
    .join("\n");
  text = tidy(text);

  // Apache-2.0 section 4(d): a NOTICE file a dependency ships has to travel
  // with the program. cargo-about does not collect them, so look in every
  // crate it resolved (the same graph, as JSON) and print what is there.
  const jsonOut = join(tmp, "rust.json");
  run(bin, ["generate", "--offline", "--manifest-path", DESKTOP_MANIFEST, ...config, "--format", "json", "--output-file", jsonOut]);
  const graph = JSON.parse(readFileSync(jsonOut, "utf8"));
  const listed = new Set(graph.crates.map(({ package: p }) => `${p.name} ${p.version}`));

  // Cross-check against what cargo says is linked: `cargo tree` over normal
  // dependencies (no dev, build or proc-macro edges) for every target.
  // cargo-about reads the same metadata but has been seen to drop a crate
  // (keyring has two optional dependencies of the same name, security-framework
  // 2 for iOS and 3 for macOS, and only the first survives). A crate that
  // ships and is not listed gets its licence files read here, from the crate
  // itself, and printed in its own section.
  const shipped = shippedCrates(readTargets());
  const missing = [...shipped].filter((c) => !listed.has(c)).sort(by);
  const metadata = missing.length
    ? JSON.parse(run("cargo", ["metadata", "--locked", "--offline", "--format-version", "1", "--manifest-path", DESKTOP_MANIFEST]))
    : { packages: [] };
  const extra = missing.map((id) => {
    const pkg = metadata.packages.find((p) => `${p.name} ${p.version}` === id);
    if (!pkg) throw new Error(`${id} ships but cargo metadata does not know it`);
    const dir = dirname(pkg.manifest_path);
    const files = readdirSync(dir)
      .filter((n) => /^(LICENSE|LICENCE|COPYING|UNLICENSE)/i.test(n) && statSync(join(dir, n)).isFile())
      .sort(by);
    if (!files.length) throw new Error(`${id} ships, is not listed by cargo-about and has no licence file to read`);
    return { pkg, dir, files: files.map((n) => ({ name: n, text: tidy(readFileSync(join(dir, n), "utf8")) })) };
  });
  const extraSection = extra.length
    ? [
        "RUST CRATES READ DIRECTLY",
        "=========================",
        "",
        "cargo-about did not list these crates, which are linked on at least one platform, so",
        "their licence files are reproduced as they are in the published crate. Where a",
        "crate offers a choice of licences, every licence file it ships is given.",
        "",
        ...extra.flatMap(({ pkg, files }) => [
          THIN,
          `${pkg.name} ${pkg.version} - declared: ${pkg.license}${pkg.repository ? ` - ${pkg.repository}` : ""}`,
          ...(pkg.authors?.length ? [`copyright: ${withoutEmails(pkg.authors.join(", "))}`] : []),
          THIN,
          "",
          ...files.flatMap((f) => [`[${f.name}]`, "", f.text, ""]),
        ]),
      ]
    : [];

  // Apache-2.0 section 4(d): a NOTICE file a dependency ships has to travel
  // with the program. cargo-about does not collect them, so look in every
  // crate it resolved and in the ones read directly.
  const dirs = [
    ...graph.crates.map(({ package: p }) => ({ id: `${p.name} ${p.version}`, dir: dirname(p.manifest_path) })),
    ...extra.map(({ pkg, dir }) => ({ id: `${pkg.name} ${pkg.version}`, dir })),
  ];
  const notices = [];
  for (const { id, dir } of dirs) {
    for (const name of readdirSync(dir).sort(by)) {
      if (/^NOTICE/i.test(name) && statSync(join(dir, name)).isFile()) {
        notices.push({ crate: id, name, text: tidy(readFileSync(join(dir, name), "utf8")) });
      }
    }
  }
  notices.sort((a, b) => by(a.crate, b.crate) || by(a.name, b.name));
  const section = [
    "NOTICE FILES OF RUST CRATES",
    "===========================",
    "",
    notices.length
      ? "The crates below ship a NOTICE file; its text is reproduced as the Apache License 2.0, section 4(d), asks."
      : "No Rust crate in this build ships a NOTICE file.",
    "",
    ...notices.flatMap((n) => [THIN, `${n.crate} - ${n.name}`, THIN, "", n.text, ""]),
  ];
  const marker = "\nSOURCE AVAILABILITY (MPL-2.0)\n";
  const at = text.indexOf(marker);
  if (at < 0) throw new Error("about.hbs lost its SOURCE AVAILABILITY (MPL-2.0) section");
  return tidy(`${text.slice(0, at)}\n\n${[...extraSection, ...(extraSection.length ? [""] : []), ...section].join("\n")}\n${text.slice(at)}`);
}

/** Does an SPDX expression ("MIT OR Apache-2.0") fit the allow-list? */
function licenceAllowed(expr) {
  const parts = expr.split(/\s+(AND|OR)\s+/);
  const ids = parts.filter((_, i) => i % 2 === 0).map((s) => s.replace(/[()]/g, "").trim());
  const ops = parts.filter((_, i) => i % 2 === 1);
  const ok = ids.map((id) => JS_ALLOWED.has(id.split(/\s+WITH\s+/)[0]));
  return ops.length && ops.every((o) => o === "OR") ? ok.some(Boolean) : ok.every(Boolean);
}

function readPackage(name) {
  const dir = join(repo, "apps/desktop/node_modules", name);
  if (!existsSync(dir)) throw new Error(`${name} is not installed: run pnpm install`);
  const pkg = JSON.parse(readFileSync(join(dir, "package.json"), "utf8"));
  const file = ["LICENSE", "LICENSE.md", "LICENSE.txt", "LICENCE", "COPYING"]
    .map((f) => join(dir, f))
    .find((f) => existsSync(f));
  if (!file) throw new Error(`${name} ships no licence file`);
  const notice = ["NOTICE", "NOTICE.md", "NOTICE.txt"].map((f) => join(dir, f)).find((f) => existsSync(f));
  return {
    name: pkg.name,
    version: pkg.version,
    license: typeof pkg.license === "string" ? pkg.license : pkg.license?.type,
    licenseText: readFileSync(file, "utf8"),
    noticeText: notice ? readFileSync(notice, "utf8") : null,
    repository: typeof pkg.repository === "string" ? pkg.repository : pkg.repository?.url,
  };
}

function frontend(tmp) {
  const json = join(tmp, "frontend.json");
  // Vite's own entry run by this node: no `pnpm` to spawn (on Windows that is
  // pnpm.cmd, which Node will not start without a shell).
  const desktop = join(repo, "apps/desktop");
  const vitePkg = createRequire(join(desktop, "package.json")).resolve("vite/package.json");
  const viteBin = join(dirname(vitePkg), JSON.parse(readFileSync(vitePkg, "utf8")).bin.vite);
  run(process.execPath, [viteBin, "build", "--outDir", join(tmp, "dist"), "--emptyOutDir", "--logLevel", "error"], {
    cwd: desktop,
    env: { ...process.env, EDYTLAB_NOTICES_JSON: json },
  });
  const bundled = JSON.parse(readFileSync(json, "utf8"));

  // Every font package the app depends on must be in the list above.
  const deps = JSON.parse(readFileSync(join(repo, "apps/desktop/package.json"), "utf8")).dependencies;
  for (const dep of Object.keys(deps)) {
    if (dep.startsWith("@fontsource") && !CSS_AND_FONT_PACKAGES.includes(dep)) {
      throw new Error(`${dep} is a font package: add it to CSS_AND_FONT_PACKAGES in generate.mjs`);
    }
  }
  const have = new Set(bundled.map((p) => p.name));
  const all = [...bundled, ...CSS_AND_FONT_PACKAGES.filter((n) => !have.has(n)).map(readPackage)];

  for (const p of all) {
    if (!p.license) throw new Error(`${p.name}@${p.version} declares no licence`);
    if (!licenceAllowed(p.license)) {
      throw new Error(
        `${p.name}@${p.version} is under "${p.license}", which is not on the allow-list in generate.mjs. ` +
          `Stop and ask the project owner before adding it.`,
      );
    }
    if (!p.licenseText || !p.licenseText.trim()) {
      throw new Error(`${p.name}@${p.version} ships no licence text for the bundle to carry`);
    }
  }

  // One block per distinct licence text, so a copyright line a package's file
  // carries is kept; packages that share a text are listed together.
  const groups = new Map();
  for (const p of all) {
    const text = tidy(p.licenseText);
    if (!groups.has(text)) groups.set(text, []);
    groups.get(text).push(p);
  }
  const cmp = (a, b) => by(a.name, b.name) || by(a.version, b.version);
  const blocks = [...groups.entries()]
    .map(([text, pkgs]) => ({ text, pkgs: pkgs.sort(cmp) }))
    .sort((a, b) => cmp(a.pkgs[0], b.pkgs[0]));

  const lines = [];
  lines.push("JAVASCRIPT PACKAGES AND FONTS", "=============================", "");
  lines.push(
    "The window of the app is a web page built from the packages below. This is",
    "what the production build contains (the JavaScript that ended up in the",
    "bundle, and the fonts and style framework it pulls in). Packages used only",
    "to build or test the app are not listed.",
    "",
    "The Geist and Geist Mono fonts are under the SIL Open Font License 1.1.",
    "They are shipped as the @fontsource-variable packages publish them, in",
    "WOFF2 form, not changed by edytlab, and neither declares a Reserved Font",
    "Name, so the licence asks nothing beyond keeping its text and copyright",
    "line with them, which follows.",
    "",
  );
  for (const { text, pkgs } of blocks) {
    lines.push(THIN);
    lines.push(`Used by:`);
    for (const p of pkgs) {
      const repoUrl = p.repository ? ` - ${String(p.repository).replace(/^git\+/, "").replace(/\.git$/, "")}` : "";
      lines.push(`  * ${p.name} ${p.version} - declared: ${p.license}${repoUrl}`);
    }
    lines.push(THIN, "", text, "");
    for (const p of pkgs) {
      if (p.noticeText && p.noticeText.trim()) {
        lines.push(`NOTICE of ${p.name} ${p.version}:`, "", tidy(p.noticeText), "");
      }
    }
  }
  return { text: tidy(lines.join("\n")), count: all.length };
}

function assemble() {
  const tmp = mkdtempSync(join(tmpdir(), "edytlab-notices-"));
  try {
    const preface = tidy(readFileSync(join(here, "preface.txt"), "utf8"));
    const credits = tidy(readFileSync(join(here, "credits.txt"), "utf8"));
    const fe = frontend(tmp);
    const rust = cargoAbout(tmp);
    return [preface, credits, fe.text, rust].join(`\n\n${RULE}\n\n`) + "\n";
  } finally {
    rmSync(tmp, { recursive: true, force: true });
  }
}

const check = process.argv.includes("--check");
const next = assemble();
if (check) {
  const current = existsSync(OUT) ? readFileSync(OUT, "utf8") : "";
  if (current !== next) {
    const fresh = join(tmpdir(), "THIRD_PARTY_NOTICES.fresh.txt");
    writeFileSync(fresh, next);
    console.error(
      "THIRD_PARTY_NOTICES.txt is out of date: a lockfile, a dependency or the notices inputs changed.\n" +
        "Regenerate it and commit it:\n\n  node scripts/third-party-notices/generate.mjs\n\n" +
        `(the fresh text was written to ${fresh} for diffing)`,
    );
    process.exit(1);
  }
  console.log("THIRD_PARTY_NOTICES.txt is up to date.");
} else {
  writeFileSync(OUT, next);
  console.log(`wrote ${OUT} (${next.length} bytes)`);
}
