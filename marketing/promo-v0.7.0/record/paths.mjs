import { lstatSync, readlinkSync, realpathSync } from "node:fs";
import { basename, dirname, isAbsolute, relative, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

export const PROJECT_ROOT = realpathSync(resolve(dirname(fileURLToPath(import.meta.url)), ".."));

/** Resolve existing ancestors and dangling links before creating an output. */
function canonicalPath(path, links = 0) {
  if (links > 40) throw new Error(`Too many symbolic links in ${path}`);
  try {
    return realpathSync(path);
  } catch (error) {
    if (error.code !== "ENOENT") throw error;
  }
  const stat = lstatSync(path, { throwIfNoEntry: false });
  if (stat?.isSymbolicLink()) return canonicalPath(resolve(dirname(path), readlinkSync(path)), links + 1);
  const parent = dirname(path);
  if (parent === path) throw new Error(`Cannot resolve ${path}`);
  return resolve(canonicalPath(parent, links), basename(path));
}

/** Promo data stays in this project; external native runs must be copied in. */
export function projectPath(path) {
  const resolved = canonicalPath(resolve(path));
  const rel = relative(PROJECT_ROOT, resolved);
  if (rel === ".." || rel.startsWith(`..${sep}`) || isAbsolute(rel)) {
    throw new Error(`Refused path ${JSON.stringify(path)}: must resolve inside ${PROJECT_ROOT}`);
  }
  return resolved;
}
