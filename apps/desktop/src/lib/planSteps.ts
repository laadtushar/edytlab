/**
 * What a plan step says, for the plan card (#481, #482).
 *
 * A step reaches the card from the model, so nothing about it can be
 * trusted to be there: the agent loop drops the steps that name nothing,
 * and this still has to draw whatever is left without ever showing a
 * bare "—".
 */

import { toolLabel } from "../components/ToolBadge";

/** One step of a plan, as the card holds it. */
export interface PlanStep {
  step: number;
  /** The tool's registered name; empty when the model named none. */
  tool: string;
  /** What the step does, in the model's words; empty when it gave none. */
  description: string;
  /**
   * The backend marked the tool as doing nothing in this build (#482):
   * the step is shown so the user sees what was proposed, and it will
   * only fail. Absent otherwise.
   */
  unavailable?: boolean;
  /**
   * Arguments the model wrote beside the step. Only used to say what a
   * step does when it gave no description.
   */
  args?: Record<string, unknown>;
}

/** What the card says of a step that can run in no build. */
export const UNAVAILABLE_TEXT = "not available in this build";

/** What the card says of a step that came with nothing to show. */
const NO_DETAILS = "No details given";

/** The keys a model puts a tool's arguments under, in the order tried. */
const ARGS_KEYS = ["args", "arguments", "input", "params", "parameters"];

/** Longest one argument value is shown at, as on the held-edit card. */
const VALUE_CAP = 80;

/** Longest the arguments are shown at, all together. */
const DETAIL_CAP = 300;

function isPlainObject(v: unknown): v is Record<string, unknown> {
  return typeof v === "object" && v !== null && !Array.isArray(v);
}

/**
 * Read a step off the `agent://plan` event.
 *
 * `index` is the step's place in the plan, used as its number when the
 * model gave none: the card used to read a missing number as 0, and keyed
 * its lines on it, so a plan with no numbers drew every line under one key.
 */
export function toPlanStep(raw: Record<string, unknown>, index: number): PlanStep {
  const step: PlanStep = {
    step: typeof raw["step"] === "number" ? raw["step"] : index + 1,
    tool: typeof raw["tool"] === "string" ? raw["tool"] : "",
    description:
      typeof raw["description"] === "string" ? raw["description"] : "",
  };
  if (raw["unavailable"] === true) step.unavailable = true;
  for (const key of ARGS_KEYS) {
    const candidate = raw[key];
    if (isPlainObject(candidate) && Object.keys(candidate).length > 0) {
      step.args = candidate;
      break;
    }
  }
  return step;
}

function shorten(text: string, max: number): string {
  return text.length <= max ? text : `${text.slice(0, max - 1)}…`;
}

/** One argument value, the way the held-edit card shows it. */
function renderValue(v: unknown): string {
  if (typeof v === "string") return JSON.stringify(shorten(v, VALUE_CAP));
  if (typeof v === "number" || typeof v === "boolean" || v === null) {
    return String(v);
  }
  if (
    Array.isArray(v) &&
    v.length <= 8 &&
    v.every((i) => typeof i !== "object" || i === null)
  ) {
    return `[${v.map(renderValue).join(", ")}]`;
  }
  return shorten(JSON.stringify(v) ?? String(v), VALUE_CAP);
}

/**
 * The step as two pieces of text for the card, drawn `label — detail`.
 *
 * `label` is the tool's friendly name, the words its badge uses. `detail`
 * is the model's description; with none, the arguments the model wrote
 * for the step; with none of those, empty, and the card then draws the
 * label alone, with no dash after it. Either can be empty, never both:
 * a step with nothing at all says so.
 */
export function describeStep(step: PlanStep): { label: string; detail: string } {
  const label = step.tool.trim() ? toolLabel(step.tool.trim()) : "";
  const described = step.description.trim();
  if (described) return { label, detail: described };

  const parts = Object.entries(step.args ?? {}).map(
    ([key, value]) => `${key}: ${renderValue(value)}`,
  );
  if (parts.length > 0) {
    return { label, detail: shorten(parts.join(", "), DETAIL_CAP) };
  }
  return label ? { label, detail: "" } : { label: NO_DETAILS, detail: "" };
}

/** Whether nothing in the plan can run in this build. */
export function nothingCanRun(steps: readonly PlanStep[]): boolean {
  return steps.length > 0 && steps.every((s) => s.unavailable === true);
}
