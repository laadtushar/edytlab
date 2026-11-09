import { lstatSync, readlinkSync, realpathSync, statSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, isAbsolute, join, parse, relative, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

const PROJECT_ROOT = realpathSync(resolve(dirname(fileURLToPath(import.meta.url)), ".."));
const REPO_ROOT = dirname(dirname(PROJECT_ROOT));

/**
 * The files outside this project that the social-kit build reads today, and no others.
 * They are read only. Each must be the regular file at exactly this path in the checkout:
 * a link anywhere below the repository root that redirects it is refused.
 */
export const SHARED_SOURCES = Object.freeze([
  // artifact/launch-hub.mjs: post copy kept in the launch and social folders.
  "marketing/launch/product-hunt.md",
  "marketing/launch/show-hn.md",
  "marketing/social/x-thread.md",
  "marketing/social/linkedin.md",
  "marketing/press-kit.md",
  // artifact/launch-hub.mjs: the site's video lists and the posters it publishes beside the page.
  "website/lib/videos.ts",
  "website/lib/demos.ts",
  ...["launch-video", "reel-highlights", "reel-beatmatched-transition", "reel-extended-club-intro",
    "reel-mini-mix", "reel-teaser"].map((slug) => `website/public/videos/${slug}.jpg`),
  ...["dj-beatmatched-transition", "dj-extended-club-intro", "dj-mini-mix"]
    .map((slug) => `website/public/demos/${slug}.jpg`),
  // linkedin/tokens.mjs (both renders): the site's tokens and logo, the app version, the tool count.
  "website/app/globals.css",
  "website/components/motion/logo.tsx",
  "website/public/logo.svg",
  "apps/desktop/src-tauri/tauri.conf.json",
  "docs/tools-reference.md",
  // images/extract-frames.sh: the recordings its app frames are cut from.
  "marketing/promo-v0.7.0/out/edytlab-v0.7.0-16x9.mp4",
  ...["dj-beatmatched-transition", "dj-extended-club-intro", "dj-mini-mix"]
    .map((slug) => `website/public/demos/${slug}.mp4`),
]);

/** Package files the renders load from apps/desktop's installed dependencies, read only. */
export const PACKAGE_SOURCES = Object.freeze({
  // linkedin/tokens.mjs: the app's mono face.
  "@fontsource-variable/geist-mono": Object.freeze(["files/geist-mono-latin-wght-normal.woff2"]),
});

/** Whether a resolved path is `root` or below it. */
function inside(root, resolved) {
  const rel = relative(root, resolved);
  return !(rel === ".." || rel.startsWith(`..${sep}`) || isAbsolute(rel));
}

/** Whether a canonical path names an existing regular file. */
function regularFile(resolved) {
  return statSync(resolved, { throwIfNoEntry: false })?.isFile() ?? false;
}

/** Follow each link before applying a later parent component, including new targets. */
function canonicalPath(destination) {
  return followLinks(destination, { links: 0 }, process.cwd());
}

/** lstat a path component; one below an existing file (ENOTDIR) reads as missing. */
function componentStat(candidate) {
  try {
    return lstatSync(candidate, { throwIfNoEntry: false });
  } catch (error) {
    // The launch builder replaces OUT even when an earlier output is a file.
    if (error.code !== "ENOTDIR") throw error;
    return undefined;
  }
}

/** canonicalPath's walk, sharing one link budget across nested link targets. */
function followLinks(destination, budget, base) {
  const { root } = parse(destination);
  let current = root ? resolve(base, root) : base;
  const separator = process.platform === "win32" ? /[\\/]/ : /\//;
  for (const component of destination.slice(root.length).split(separator)) {
    if (!component || component === ".") continue;
    if (component === "..") {
      current = dirname(current);
      continue;
    }
    const candidate = join(current, component);
    const stat = componentStat(candidate);
    if (stat?.isSymbolicLink()) {
      if (++budget.links > 40) throw new Error(`Too many symbolic links in ${destination}`);
      current = followLinks(readlinkSync(candidate), budget, current);
    } else {
      current = candidate;
    }
  }
  return current;
}

/**
 * Refuse outputs whose existing or future targets escape the social project.
 * Keep lexical operation paths: cleanup must remove an OUT link, not its target.
 */
export function checkOutputPath(destination) {
  const resolved = canonicalPath(destination);
  if (!inside(PROJECT_ROOT, resolved)) {
    throw new Error(`Refused path ${JSON.stringify(destination)}: must resolve inside ${PROJECT_ROOT}`);
  }
}

/**
 * Refuse an input unless it resolves inside the social project or is one of
 * SHARED_SOURCES at its own canonical path. Returns the path unchanged.
 */
export function checkInputPath(source) {
  const lexical = resolve(source);
  const resolved = canonicalPath(lexical);
  if (inside(PROJECT_ROOT, resolved)) return source;
  const shared = SHARED_SOURCES.some((entry) => lexical === join(REPO_ROOT, entry));
  if (shared && resolved === lexical && regularFile(resolved)) return source;
  throw new Error(`Refused input ${JSON.stringify(source)}: must resolve inside ${PROJECT_ROOT} `
    + "or be an allowlisted shared source at its own path");
}

/**
 * The canonical path of one PACKAGE_SOURCES file, found as Node finds it from
 * apps/desktop. It must be a regular file in this checkout's installed packages.
 */
export function packageSourcePath(specifier, file) {
  if (!Object.hasOwn(PACKAGE_SOURCES, specifier) || !PACKAGE_SOURCES[specifier].includes(file)) {
    throw new Error(`Refused input ${JSON.stringify(`${specifier}/${file}`)}: not an allowlisted package source`);
  }
  const desktop = join(REPO_ROOT, "apps", "desktop");
  const lexical = join(dirname(createRequire(join(desktop, "package.json")).resolve(`${specifier}/package.json`)), file);
  const resolved = canonicalPath(lexical);
  const installed = [join(REPO_ROOT, "node_modules"), join(desktop, "node_modules")];
  if (resolved !== lexical || !installed.some((root) => inside(root, resolved))
      || !resolved.endsWith(sep + join("node_modules", specifier, file)) || !regularFile(resolved)) {
    throw new Error(`Refused input ${JSON.stringify(lexical)}: package source must resolve to `
      + `${join("node_modules", specifier, file)} inside this checkout's installed packages`);
  }
  return resolved;
}
