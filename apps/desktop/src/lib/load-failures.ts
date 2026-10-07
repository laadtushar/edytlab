/**
 * What the user is told when `batch_load` could not add some files.
 *
 * The backend reports each refused file with the tool's own reason, and
 * keeps loading the rest. The banner has to name the files — "could not
 * load" with no name is useless after a five-file drop — without turning
 * into a wall of text when most of them fail.
 */

export interface LoadFailure {
  path: string;
  error: string;
}

/** Longest list of files spelled out before the rest are counted. */
const MAX_NAMED = 3;

function baseName(path: string): string {
  const parts = path.split(/[\\/]/);
  return parts[parts.length - 1] || path;
}

/** One line for the error banner, or `null` when nothing failed. */
export function describeLoadFailures(failures: readonly LoadFailure[]): string | null {
  if (failures.length === 0) return null;
  if (failures.length === 1) {
    const [f] = failures;
    return `Could not load ${baseName(f.path)}: ${f.error}`;
  }
  const named = failures
    .slice(0, MAX_NAMED)
    .map((f) => `${baseName(f.path)} (${f.error})`)
    .join("; ");
  const more = failures.length - MAX_NAMED;
  return `Could not load ${failures.length} files: ${named}${more > 0 ? `; and ${more} more` : ""}`;
}
