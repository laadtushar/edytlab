/** Sidecar notes for a shot: what was recorded, for the README and the edit. */

import { writeFileSync } from "node:fs";
import { join } from "node:path";

import { FOOTAGE_DIR } from "./paths";

export function writeNotes(name: string, notes: Record<string, unknown>): void {
  // Paths are written relative to the footage folder, so the notes read the same anywhere.
  const rounded = JSON.parse(
    JSON.stringify(notes, (_k, v) =>
      typeof v === "number" ? Number(v.toFixed(3)) : typeof v === "string" ? v.replace(`${FOOTAGE_DIR}/`, "") : v,
    ),
  );
  writeFileSync(join(FOOTAGE_DIR, `${name}.notes.json`), JSON.stringify(rounded, null, 2) + "\n");
}
