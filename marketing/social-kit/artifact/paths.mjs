import { lstatSync, readlinkSync, realpathSync } from "node:fs";
import { basename, dirname, isAbsolute, relative, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

const PROJECT_ROOT = realpathSync(resolve(dirname(fileURLToPath(import.meta.url)), ".."));

/** Resolve existing parents and dangling links before a destination is created. */
function canonicalPath(destination, links = 0) {
  if (links > 40) throw new Error(`Too many symbolic links in ${destination}`);
  try {
    return realpathSync(destination);
  } catch (error) {
    // The launch builder replaces OUT even when an earlier output is a file.
    if (error.code !== "ENOENT" && error.code !== "ENOTDIR") throw error;
  }
  let stat;
  try {
    stat = lstatSync(destination, { throwIfNoEntry: false });
  } catch (error) {
    if (error.code !== "ENOTDIR") throw error;
  }
  if (stat?.isSymbolicLink()) {
    return canonicalPath(resolve(dirname(destination), readlinkSync(destination)), links + 1);
  }
  const parent = dirname(destination);
  if (parent === destination) throw new Error(`Cannot resolve ${destination}`);
  return resolve(canonicalPath(parent, links), basename(destination));
}

/**
 * Refuse outputs whose existing or future targets escape the social project.
 * Keep lexical operation paths: cleanup must remove an OUT link, not its target.
 */
export function checkOutputPath(destination) {
  const resolved = canonicalPath(resolve(destination));
  const rel = relative(PROJECT_ROOT, resolved);
  if (rel === ".." || rel.startsWith(`..${sep}`) || isAbsolute(rel)) {
    throw new Error(`Refused path ${JSON.stringify(destination)}: must resolve inside ${PROJECT_ROOT}`);
  }
}
