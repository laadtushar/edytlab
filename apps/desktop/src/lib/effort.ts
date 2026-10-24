/**
 * The reasoning-effort choices, shared by the API-key providers' picker and
 * the ChatGPT plan's (#474).
 *
 * The order is the one the API ranks them in. The empty value is "Default":
 * nothing is sent and the model uses its own.
 */
import type { ReasoningEffort } from "./tauri-bridge";

export const EFFORT_OPTIONS: ReadonlyArray<{
  value: ReasoningEffort | "";
  label: string;
}> = [
  { value: "", label: "Default (model's own)" },
  { value: "low", label: "Low" },
  { value: "medium", label: "Medium" },
  { value: "high", label: "High" },
  { value: "xhigh", label: "Extra high" },
  { value: "max", label: "Max" },
];

/** `value` as a level this app can save, or "" for anything else. */
export function asEffort(value: string | null | undefined): ReasoningEffort | "" {
  return EFFORT_OPTIONS.some((o) => o.value === value && o.value !== "")
    ? (value as ReasoningEffort)
    : "";
}

/**
 * The levels to offer for a ChatGPT plan model: the ones it lists
 * (`ModelInfo.reasoning_levels`) that the backend can save, in the API's
 * order. A level this app does not know cannot be saved
 * (`set_effort_for` refuses it), so it is not offered. Empty when the model
 * lists none, which is also the model for which the backend sends no effort.
 */
export function levelsOffered(listed: readonly string[]): ReasoningEffort[] {
  return EFFORT_OPTIONS.map((o) => o.value)
    .filter((v): v is ReasoningEffort => v !== "")
    .filter((v) => listed.includes(v));
}
