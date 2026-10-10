/**
 * What a plan step says (#481, #482).
 *
 * The plan card drew `{tool} — {description}` for every step, whatever the
 * model had written. A step whose keys did not match the schema reached the
 * card with both empty and read as a bare "—"; one with a tool and no words
 * read "add_marker —". These pin the text a step turns into, away from React.
 */

import { describe, expect, it } from "vitest";

import { describeStep, toPlanStep } from "../lib/planSteps";

describe("toPlanStep", () => {
  it("reads a step that follows the schema as it is", () => {
    expect(
      toPlanStep(
        { step: 2, tool: "label", description: "Add marker 'Drop' at 16.0 s" },
        1,
      ),
    ).toEqual({
      step: 2,
      tool: "label",
      description: "Add marker 'Drop' at 16.0 s",
    });
  });

  it("turns missing and mistyped keys into empty text, never into 'undefined'", () => {
    const step = toPlanStep(
      { step: "two", tool: 7, description: { text: "no" } },
      3,
    );
    expect(step.tool).toBe("");
    expect(step.description).toBe("");
    // A step number the model did not give is its place in the plan, not 0:
    // the card keys its lines on it.
    expect(step.step).toBe(4);
  });

  it("carries the backend's mark that the tool does nothing in this build", () => {
    expect(
      toPlanStep({ step: 1, tool: "separate_stems", unavailable: true }, 0)
        .unavailable,
    ).toBe(true);
    // Only a literal `true` is the mark.
    expect(
      toPlanStep({ step: 1, tool: "reverse", unavailable: "yes" }, 0)
        .unavailable,
    ).toBeUndefined();
    expect(
      toPlanStep({ step: 1, tool: "reverse" }, 0).unavailable,
    ).toBeUndefined();
  });

  it("keeps the arguments a model wrote beside the step, under any of the usual names", () => {
    for (const key of ["args", "arguments", "input", "params", "parameters"]) {
      expect(
        toPlanStep({ step: 1, tool: "label", [key]: { time: 16 } }, 0).args,
        key,
      ).toEqual({ time: 16 });
    }
    // Not arguments: a list, a string, an empty object.
    for (const junk of [[1, 2], "time=16", {}, null]) {
      expect(
        toPlanStep({ step: 1, tool: "label", args: junk }, 0).args,
        JSON.stringify(junk),
      ).toBeUndefined();
    }
  });
});

describe("describeStep", () => {
  it("shows the model's description after the tool's friendly name", () => {
    expect(
      describeStep({
        step: 1,
        tool: "label",
        description: "Add marker 'Drop' at 16.0 s",
      }),
    ).toEqual({ label: "Add marker", detail: "Add marker 'Drop' at 16.0 s" });
  });

  it("falls back to the friendly name with the key arguments when there is no description", () => {
    expect(
      describeStep({
        step: 1,
        tool: "label",
        description: "",
        args: { time: 16, name: "Drop" },
      }),
    ).toEqual({ label: "Add marker", detail: 'time: 16, name: "Drop"' });
  });

  it("treats a description of only whitespace as none", () => {
    expect(
      describeStep({
        step: 1,
        tool: "label",
        description: "  \n ",
        args: { time: 16 },
      }).detail,
    ).toBe("time: 16");
  });

  it("is the friendly name alone, with nothing to hang a dash on, when it has neither", () => {
    expect(
      describeStep({ step: 1, tool: "label", description: "" }),
    ).toEqual({ label: "Add marker", detail: "" });
  });

  it("uses the label the badges use, and the badges' fallback for a tool they do not know", () => {
    expect(describeStep({ step: 1, tool: "de_esser", description: "" }).label).toBe(
      "De-esser",
    );
    expect(describeStep({ step: 1, tool: "made_up_tool", description: "" }).label).toBe(
      "made up tool",
    );
  });

  it("is just the words when the model named no tool", () => {
    expect(
      describeStep({
        step: 1,
        tool: "",
        description: "Listen back to the result",
      }),
    ).toEqual({ label: "", detail: "Listen back to the result" });
  });

  it("says so, in words, when a step has nothing at all", () => {
    const { label, detail } = describeStep({
      step: 1,
      tool: "",
      description: "",
    });
    // Never an empty line, and never a bare dash.
    expect(`${label}${detail}`.trim()).not.toBe("");
    expect(`${label} ${detail}`).not.toMatch(/^\s*[—-]\s*$/);
  });

  it("renders argument values the way the held-edit card does", () => {
    const { detail } = describeStep({
      step: 1,
      tool: "reverb",
      description: "",
      args: {
        track: 0,
        wet: 0.4,
        name: "Hall",
        on: true,
        off: null,
        range: [24, 32],
        nested: { a: 1 },
      },
    });
    expect(detail).toBe(
      'track: 0, wet: 0.4, name: "Hall", on: true, off: null, range: [24, 32], nested: {"a":1}',
    );
  });

  it("cuts a long value rather than filling the card with it", () => {
    const { detail } = describeStep({
      step: 1,
      tool: "label",
      description: "",
      args: { name: "x".repeat(500) },
    });
    expect(detail.length).toBeLessThan(200);
    expect(detail.endsWith('…"')).toBe(true);
  });
});
