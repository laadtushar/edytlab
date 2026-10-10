/**
 * The words of one plan step: `Add marker — Add marker 'Drop' at 16.0 s`.
 *
 * Drawn by both the approval card and the transcript entry that stays
 * after the card is answered. Each had its own copy of
 * `{tool} — {description}`, and both drew a bare "—" for a step that came
 * with no words (#481). A step whose tool does nothing in this build says
 * so in words (#482), not only in colour.
 */

import { describeStep, UNAVAILABLE_TEXT, type PlanStep } from "../lib/planSteps";

export function PlanStepText({ step }: { step: PlanStep }) {
  const { label, detail } = describeStep(step);
  const unavailable = step.unavailable === true;
  return (
    <span data-testid="plan-step-text">
      <span
        // The registered name, for a user who wants to know which tool.
        title={step.tool || undefined}
        className={
          "font-mono " +
          (unavailable
            ? "text-[var(--text-dim)] line-through"
            : "text-[var(--text)]")
        }
      >
        {label}
      </span>
      {detail ? (
        <span className={unavailable ? "text-[var(--text-faint)]" : undefined}>
          {label ? " — " : ""}
          {detail}
        </span>
      ) : null}
      {unavailable ? (
        <span
          data-testid="plan-step-unavailable"
          className="
            ml-1.5 whitespace-nowrap rounded border border-[var(--danger)]/40
            px-1.5 py-0.5 text-[10px] text-[var(--danger)]
          "
        >
          {UNAVAILABLE_TEXT}
        </span>
      ) : null}
    </span>
  );
}
