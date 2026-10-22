/**
 * The plan card says what each step will do (#481, #482).
 *
 * Plan steps used to reach the card as `{tool} — {description}` whatever
 * the model had written. A step whose keys did not match the schema drew
 * as a bare "—", one with a tool and no words as "add_marker —", and
 * pressing Run approved something the user could not read. A step whose
 * tool does nothing in this build drew like any other, and Run offered it.
 *
 * This drives the real listener through the bridge and reads what the
 * card draws, because `Chat` renders the plan in two places (the card, and
 * the transcript entry that stays after it is answered) and each had its
 * own copy of the line.
 *
 * Every wait is for the thing under test: the listener being subscribed
 * before the event is sent, and the card being drawn before it is read.
 * A fixed delay passes when React is fast and fails when it is not
 * (`test:slow-scheduler`, #349).
 */

import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

const { cbs, noop, approvePlanMock } = vi.hoisted(() => ({
  cbs: {
    plan: [] as ((steps: Record<string, unknown>[]) => void)[],
  },
  noop: () => Promise.resolve(() => undefined),
  approvePlanMock: vi.fn((_steps?: unknown) => Promise.resolve()),
}));

vi.mock("../lib/tauri-bridge", () => ({
  sendMessage: vi.fn(() => Promise.resolve()),
  approvePlan: (steps?: unknown) => approvePlanMock(steps),
  rejectPlan: vi.fn(() => Promise.resolve()),
  getPlanFirst: vi.fn(() => Promise.resolve(true)),
  setPlanFirst: vi.fn(() => Promise.resolve()),
  listCapabilities: vi.fn(() =>
    Promise.resolve({ tools: [], skills: [], agents: [], mcp_servers: [] }),
  ),
  onTextDelta: vi.fn(noop),
  onToolCall: vi.fn(noop),
  onToolCallEnd: vi.fn(noop),
  onNodeCreated: vi.fn(noop),
  onAgentDone: vi.fn(noop),
  onPlan: vi.fn((cb: (steps: Record<string, unknown>[]) => void) => {
    cbs.plan.push(cb);
    return Promise.resolve(() => undefined);
  }),
  onPlanUnavailable: vi.fn(noop),
  onPlanRejected: vi.fn(noop),
}));

import { Chat } from "../components/Chat";

/** Mount the chat, wait until it listens for plans, and send it `steps`. */
async function showPlan(steps: Record<string, unknown>[]) {
  render(<Chat />);
  await waitFor(() => expect(cbs.plan.length).toBeGreaterThan(0));
  await act(async () => {
    cbs.plan.forEach((cb) => cb(steps));
  });
  return screen.findByTestId("plan-approval-card");
}

/**
 * The words of each step line on the card, whitespace collapsed. The
 * line's own Edit button is not part of them.
 */
function lines(card: HTMLElement): string[] {
  return within(card)
    .getAllByTestId("plan-step")
    .map((li) =>
      (within(li).getByTestId("plan-step-text").textContent ?? "")
        .replace(/\s+/g, " ")
        .trim(),
    );
}

describe("the plan card", () => {
  beforeEach(() => {
    cbs.plan = [];
    approvePlanMock.mockClear();
  });

  it("shows the description, under the tool's friendly name", async () => {
    const card = await showPlan([
      { step: 1, tool: "label", description: "Add marker 'Drop' at 16.0 s" },
      {
        step: 2,
        tool: "label",
        description: "Add region 'Outro' from 24.0 s to 32.0 s",
      },
    ]);
    const [first, second] = lines(card);
    expect(first).toContain("Add marker — Add marker 'Drop' at 16.0 s");
    expect(second).toContain("Add region 'Outro' from 24.0 s to 32.0 s");
    // The tool's internal name is not what the user reads.
    expect(card.textContent).not.toContain("label —");
  });

  it("never draws a bare dash for a step that came with nothing", async () => {
    // What Opus 5.5 and Haiku 5.5 sent: steps whose keys did not match.
    const card = await showPlan([
      { step: 1 },
      { action: "Edit", target: "track 0" },
      { step: 3, tool: "", description: "" },
    ]);
    for (const line of lines(card)) {
      expect(line, "a step drew as nothing").not.toBe("");
      expect(line).not.toMatch(/^[—-]$/);
      expect(line).not.toMatch(/—\s*$/);
    }
  });

  it("shows the friendly name with its arguments when there are no words", async () => {
    const card = await showPlan([
      { step: 1, tool: "label", args: { time: 16, name: "Drop" } },
      { step: 2, tool: "reverse", description: "" },
    ]);
    const [first, second] = lines(card);
    expect(first).toContain('Add marker — time: 16, name: "Drop"');
    // Nothing to follow the name, so no dash after it either.
    expect(second).toBe("Reverse");
  });

  it("shows a step with words and no tool as just the words", async () => {
    const card = await showPlan([
      { step: 1, description: "Listen back to the result" },
    ]);
    expect(lines(card)).toEqual(["Listen back to the result"]);
  });

  it("keeps one line per step when the model numbered them badly", async () => {
    // Every step 0 or missing used to share one React key.
    const card = await showPlan([
      { tool: "reverse", description: "track: 0" },
      { tool: "reverse", description: "track: 1" },
      { step: 0, tool: "reverse", description: "track: 2" },
    ]);
    expect(lines(card)).toEqual([
      "Reverse — track: 0",
      "Reverse — track: 1",
      "Reverse — track: 2",
    ]);
  });

  it("marks a step whose tool does nothing in this build, and says it will fail", async () => {
    const card = await showPlan([
      {
        step: 1,
        tool: "separate_stems",
        description: "Split track 0",
        unavailable: true,
      },
      { step: 2, tool: "analyze_track", description: "Check the result" },
    ]);
    const [stems, analyze] = within(card).getAllByTestId("plan-step");
    expect(stems).toHaveAttribute("data-unavailable", "true");
    expect(stems.textContent).toContain("not available in this build");
    expect(stems.textContent).toContain("Split track 0");
    expect(analyze).not.toHaveAttribute("data-unavailable");
    expect(analyze.textContent).not.toContain("not available");
    // There is nothing to edit in a step that cannot run.
    expect(within(stems).queryByTestId("plan-edit-button")).toBeNull();
    expect(within(analyze).getByTestId("plan-edit-button")).toBeInTheDocument();
  });

  it("offers Run when something in the plan can run", async () => {
    const card = await showPlan([
      { step: 1, tool: "separate_stems", description: "Split", unavailable: true },
      { step: 2, tool: "analyze_track", description: "Check" },
    ]);
    expect(within(card).getByTestId("plan-run-button")).toBeEnabled();
  });

  it("does not offer Run on a plan that cannot run at all, and says why", async () => {
    const card = await showPlan([
      { step: 1, tool: "separate_stems", description: "Split", unavailable: true },
      { step: 2, tool: "separate_stems", unavailable: true },
    ]);
    expect(within(card).getByTestId("plan-run-button")).toBeDisabled();
    expect(card.textContent).toContain("Nothing in this plan can run");
    // Discard is how the user leaves it, so it stays.
    expect(within(card).getByTestId("plan-discard-button")).toBeEnabled();
  });

  it("approves a step whose words came from its tool as it was, not as an edit", async () => {
    const user = userEvent.setup();
    const card = await showPlan([
      { step: 1, tool: "label", args: { time: 16, name: "Drop" } },
    ]);
    await user.click(within(card).getByTestId("plan-run-button"));
    await waitFor(() => expect(approvePlanMock).toHaveBeenCalledTimes(1));
    // No steps sent: the model is not told the user rewrote anything.
    expect(approvePlanMock).toHaveBeenCalledWith(undefined);
  });

  it("keeps the same text in the transcript once the card is answered", async () => {
    const user = userEvent.setup();
    const card = await showPlan([
      { step: 1, tool: "label", args: { time: 16, name: "Drop" } },
      { step: 2, tool: "separate_stems", unavailable: true },
    ]);
    await user.click(within(card).getByTestId("plan-run-button"));
    await waitFor(() =>
      expect(screen.queryByTestId("plan-approval-card")).toBeNull(),
    );
    const entry = await screen.findByTestId("plan-history-entry");
    const text = (entry.textContent ?? "").replace(/\s+/g, " ");
    expect(text).toContain('Add marker — time: 16, name: "Drop"');
    expect(text).toContain("Separate stems");
    expect(text).toContain("not available in this build");
    expect(text).not.toMatch(/—\s*(Separate|$)/);
  });
});
