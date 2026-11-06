import { lstatSync, readlinkSync, realpathSync } from "node:fs";
import { dirname, isAbsolute, join, parse, relative, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

const PROJECT_ROOT = realpathSync(resolve(dirname(fileURLToPath(import.meta.url)), ".."));

/** Follow each link before applying a later parent component, including new targets. */
function canonicalPath(destination, budget = { links: 0 }, base = process.cwd()) {
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
    let stat;
    try {
      stat = lstatSync(candidate, { throwIfNoEntry: false });
    } catch (error) {
      // The launch builder replaces OUT even when an earlier output is a file.
      if (error.code !== "ENOTDIR") throw error;
    }
    if (stat?.isSymbolicLink()) {
      if (++budget.links > 40) throw new Error(`Too many symbolic links in ${destination}`);
      current = canonicalPath(readlinkSync(candidate), budget, current);
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
  const rel = relative(PROJECT_ROOT, resolved);
  if (rel === ".." || rel.startsWith(`..${sep}`) || isAbsolute(rel)) {
    throw new Error(`Refused path ${JSON.stringify(destination)}: must resolve inside ${PROJECT_ROOT}`);
  }
}
