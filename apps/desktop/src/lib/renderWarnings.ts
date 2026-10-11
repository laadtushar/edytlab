/**
 * What the window says when a render leaves a stored effect out (#532).
 *
 * The backend reports each one as data (`RenderWarning`: where it sits,
 * its kind, and why); the words are here, so the sentence a person reads
 * can change without touching Rust. The agent reads the engine's own
 * sentence for the same finding; this one is for the person at the window.
 *
 * Every function returns `null` for an empty list, so a caller that has
 * nothing to say adds nothing to the screen.
 */

import type { RenderWarning, RenderWarningScope } from "./tauri-bridge";

/** A notice: a lead line, and, when there are several, one line each. */
export interface WarningNotice {
  lead: string;
  items: string[];
}

/** `noise_gate` read as `Noise gate`. */
function effectName(kind: string): string {
  const words = kind.replace(/_/g, " ").trim();
  return words.charAt(0).toUpperCase() + words.slice(1);
}

/**
 * Where, the way the agent's tools number it: a track by the index its
 * `track` argument takes (from 0) and its name, so what the window says
 * matches what the assistant says.
 */
export function scopeLabel(scope: RenderWarningScope): string {
  switch (scope.type) {
    case "track":
      return scope.name ? `track ${scope.index} "${scope.name}"` : `track ${scope.index}`;
    case "bus":
      return scope.name ? `bus "${scope.name}"` : "a bus";
    case "master":
      return "the master bus";
  }
}

/** `Reverb on track 0 "Vocals"`; a kind nobody knows is quoted as stored. */
function subject(w: RenderWarning): string {
  const what = w.reason === "unknown" ? `Effect "${w.kind}"` : effectName(w.kind);
  return `${what} on ${scopeLabel(w.scope)}`;
}

function reason(w: RenderWarning): string {
  return w.reason === "unknown"
    ? "this build doesn't know that effect"
    : "this build can't run it in a chain yet";
}

/** One warning as a line of a list. */
function line(w: RenderWarning): string {
  return `${subject(w)}: ${reason(w)}.`;
}

function count(n: number): string {
  return n === 1 ? "1 effect" : `${n} effects`;
}

/**
 * What Play (and Preview) leave out of the mix, for the node shown. One
 * warning is a sentence; several are a count with the list under it.
 */
export function mixNotice(warnings: readonly RenderWarning[]): WarningNotice | null {
  if (warnings.length === 0) return null;
  if (warnings.length === 1) {
    const [w] = warnings;
    return {
      lead: `${subject(w)} is left out: ${reason(w)}. Remove or bypass it to clear this.`,
      items: [],
    };
  }
  return {
    lead: `${warnings.length} effects are left out of the mix. Remove or bypass them to clear this.`,
    items: warnings.map(line),
  };
}

/** What an export wrote its file without. */
export function exportNotice(
  file: string,
  warnings: readonly RenderWarning[],
): WarningNotice | null {
  if (warnings.length === 0) return null;
  if (warnings.length === 1) {
    return { lead: `Exported ${file} without ${line(warnings[0])}`, items: [] };
  }
  return {
    lead: `Exported ${file} without ${count(warnings.length)} it stores:`,
    items: warnings.map(line),
  };
}

/**
 * What the two sides of an A/B compare leave out. A side with a reverb it
 * cannot play sounds the same as one without, which is what a comparison of
 * the two is least able to afford, so both are listed and told apart.
 */
export function compareNotice(
  a: readonly RenderWarning[],
  b: readonly RenderWarning[],
): WarningNotice | null {
  if (a.length + b.length === 0) return null;
  return {
    lead: "Effects left out of the renders being compared:",
    items: [...a.map((w) => `A · ${line(w)}`), ...b.map((w) => `B · ${line(w)}`)],
  };
}
