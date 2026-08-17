/**
 * PlanCard — tests for the mashup plan approval card rendered inside Chat.
 *
 * The bridge is mocked so we can drive plan events directly and assert
 * on the approval card rendering + the Run/Edit button behaviour.
 */

import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

// -------------------------------------------------------------------------
// Bridge mock — capture event callbacks so we can fire them in tests.
// -------------------------------------------------------------------------

const cbs = {
  textDelta: [] as ((text: string) => void)[],
  toolCall: [] as ((name: string, id: string) => void)[],
  toolCallEnd: [] as ((id: string, ok: boolean) => void)[],
  nodeCreated: [] as ((nodeId: string) => void)[],
  done: [] as (() => void)[],
  plan: [] as ((steps: Record<string, unknown>[]) => void)[],
};

const approvePlanMock = vi.fn();
const rejectPlanMock = vi.fn();
const getPlanFirstMock = vi.fn();
const setPlanFirstMock = vi.fn();
const sendMessageMock = vi.fn();

vi.mock("../../lib/tauri-bridge", () => ({
  sendMessage: (text: string) => sendMessageMock(text),
  approvePlan: (steps?: string[]) => approvePlanMock(steps),
  rejectPlan: () => rejectPlanMock(),
  getPlanFirst: () => getPlanFirstMock(),
  setPlanFirst: (on: boolean) => setPlanFirstMock(on),
  onTextDelta: vi.fn((cb: (t: string) => void) => {
    cbs.textDelta.push(cb);
    return Promise.resolve(() => undefined);
  }),
  onToolCall: vi.fn((cb: (n: string, i: string) => void) => {
    cbs.toolCall.push(cb);
    return Promise.resolve(() => undefined);
  }),
  onToolCallEnd: vi.fn((cb: (id: string, ok: boolean) => void) => {
    cbs.toolCallEnd.push(cb);
    return Promise.resolve(() => undefined);
  }),
  onNodeCreated: vi.fn((cb: (n: string) => void) => {
    cbs.nodeCreated.push(cb);
    return Promise.resolve(() => undefined);
  }),
  onAgentDone: vi.fn((cb: () => void) => {
    cbs.done.push(cb);
    return Promise.resolve(() => undefined);
  }),
  onPlan: vi.fn((cb: (steps: Record<string, unknown>[]) => void) => {
    cbs.plan.push(cb);
    return Promise.resolve(() => undefined);
  }),
}));

import { Chat } from "../Chat";

const flush = () => new Promise<void>((r) => setTimeout(r, 0));

const sampleSteps: Record<string, unknown>[] = [
  { step: 1, tool: "analyze_track", description: "Analyse A BPM and key" },
  { step: 2, tool: "analyze_track", description: "Analyse B BPM and key" },
  { step: 3, tool: "separate_stems", description: "Separate A into 4 stems" },
  { step: 4, tool: "time_stretch", description: "Stretch B to match A BPM" },
  { step: 5, tool: "render_final", description: "Render mashup" },
];

describe("PlanCard (inside Chat)", () => {
  beforeEach(() => {
    approvePlanMock.mockReset();
    rejectPlanMock.mockReset().mockResolvedValue(undefined);
    getPlanFirstMock.mockReset().mockResolvedValue(false);
    setPlanFirstMock.mockReset().mockResolvedValue(undefined);
    approvePlanMock.mockResolvedValue(undefined);
    sendMessageMock.mockReset();
    sendMessageMock.mockResolvedValue(undefined);
    cbs.textDelta = [];
    cbs.toolCall = [];
    cbs.toolCallEnd = [];
    cbs.nodeCreated = [];
    cbs.done = [];
    cbs.plan = [];
  });

  it("does NOT render the plan approval card when there is no pending plan", () => {
    render(<Chat />);
    expect(screen.queryByTestId("plan-approval-card")).not.toBeInTheDocument();
  });

  it("renders the plan approval card with correct step count when a plan event arrives", async () => {
    render(<Chat />);
    await act(async () => {
      await flush();
    });

    await act(async () => {
      cbs.plan[0](sampleSteps);
    });

    const card = screen.getByTestId("plan-approval-card");
    expect(card).toBeInTheDocument();
    // Header mentions step count
    expect(card.textContent).toContain("5 steps");
  });

  it("renders all plan steps in the approval card", async () => {
    render(<Chat />);
    await act(async () => {
      await flush();
    });

    await act(async () => {
      cbs.plan[0](sampleSteps);
    });

    // The approval card shows steps; there may be multiple matches (log entry
    // + approval card), but at least one should exist.
    expect(
      screen.getAllByText(/Analyse A BPM and key/).length,
    ).toBeGreaterThan(0);
    expect(
      screen.getAllByText(/Separate A into 4 stems/).length,
    ).toBeGreaterThan(0);
    expect(screen.getAllByText(/Render mashup/).length).toBeGreaterThan(0);
  });

  it("clicking Run calls approvePlan with no steps when unedited", async () => {
    const user = userEvent.setup();
    render(<Chat />);
    await act(async () => {
      await flush();
    });

    await act(async () => {
      cbs.plan[0](sampleSteps);
    });

    await user.click(screen.getByTestId("plan-run-button"));
    expect(approvePlanMock).toHaveBeenCalledTimes(1);
    // No edits — the bridge receives undefined so the backend gets null.
    expect(approvePlanMock).toHaveBeenCalledWith(undefined);
  });

  it("clicking Run after editing a step forwards the updated descriptions", async () => {
    const user = userEvent.setup();
    render(<Chat />);
    await act(async () => {
      await flush();
    });

    await act(async () => {
      cbs.plan[0](sampleSteps);
    });

    // Edit the first step
    const editButtons = screen.getAllByTestId("plan-edit-button");
    await user.click(editButtons[0]);
    const editor = screen.getByTestId("plan-step-editor");
    await user.clear(editor);
    await user.type(editor, "Revised first step");
    await user.click(screen.getByTestId("plan-step-save"));

    // Click Run — should forward the edited descriptions
    await user.click(screen.getByTestId("plan-run-button"));
    expect(approvePlanMock).toHaveBeenCalledTimes(1);
    const calledWith: string[] = approvePlanMock.mock.calls[0][0];
    expect(calledWith[0]).toBe("Revised first step");
    // Other steps unchanged
    expect(calledWith[1]).toBe("Analyse B BPM and key");
    expect(calledWith[2]).toBe("Separate A into 4 stems");
  });

  it("Save button is disabled when the textarea is empty", async () => {
    const user = userEvent.setup();
    render(<Chat />);
    await act(async () => {
      await flush();
    });

    await act(async () => {
      cbs.plan[0](sampleSteps);
    });

    const editButtons = screen.getAllByTestId("plan-edit-button");
    await user.click(editButtons[0]);
    const editor = screen.getByTestId("plan-step-editor");
    await user.clear(editor);

    const saveBtn = screen.getByTestId("plan-step-save");
    expect(saveBtn).toBeDisabled();
  });

  it("clicking Edit on a step opens inline textarea for that step", async () => {
    const user = userEvent.setup();
    render(<Chat />);
    await act(async () => {
      await flush();
    });

    await act(async () => {
      cbs.plan[0](sampleSteps);
    });

    // Click the first step's Edit button
    const editButtons = screen.getAllByTestId("plan-edit-button");
    await user.click(editButtons[0]);

    // Inline editor should now be visible with the step's description pre-filled
    const editor = screen.getByTestId("plan-step-editor");
    expect(editor).toBeInTheDocument();
    expect((editor as HTMLTextAreaElement).value).toBe("Analyse A BPM and key");
  });

  it("saving an edited step updates the displayed description", async () => {
    const user = userEvent.setup();
    render(<Chat />);
    await act(async () => {
      await flush();
    });

    await act(async () => {
      cbs.plan[0](sampleSteps);
    });

    const editButtons = screen.getAllByTestId("plan-edit-button");
    await user.click(editButtons[0]);

    const editor = screen.getByTestId("plan-step-editor");
    await user.clear(editor);
    await user.type(editor, "Updated step description");
    await user.click(screen.getByTestId("plan-step-save"));

    // Editor should be gone; updated text should appear
    expect(screen.queryByTestId("plan-step-editor")).not.toBeInTheDocument();
    expect(screen.getAllByText(/Updated step description/).length).toBeGreaterThan(0);
  });

  it("cancelling an edit reverts to original description", async () => {
    const user = userEvent.setup();
    render(<Chat />);
    await act(async () => {
      await flush();
    });

    await act(async () => {
      cbs.plan[0](sampleSteps);
    });

    const editButtons = screen.getAllByTestId("plan-edit-button");
    await user.click(editButtons[0]);

    const editor = screen.getByTestId("plan-step-editor");
    await user.clear(editor);
    await user.type(editor, "Discard this");
    await user.click(screen.getByTestId("plan-step-cancel"));

    expect(screen.queryByTestId("plan-step-editor")).not.toBeInTheDocument();
    expect(screen.getAllByText(/Analyse A BPM and key/).length).toBeGreaterThan(0);
  });

  it("plan approval card disappears after Run is clicked", async () => {
    const user = userEvent.setup();
    render(<Chat />);
    await act(async () => {
      await flush();
    });

    await act(async () => {
      cbs.plan[0](sampleSteps);
    });

    expect(screen.getByTestId("plan-approval-card")).toBeInTheDocument();

    await act(async () => {
      await user.click(screen.getByTestId("plan-run-button"));
      // Let the async approvePlan + state update settle.
      await flush();
    });

    expect(screen.queryByTestId("plan-approval-card")).not.toBeInTheDocument();
  });

  /**
   * Before this, the only exits from the plan gate were approving it and
   * waiting out a five-minute timeout — so a user who disliked the plan
   * had no way to say so, and the gate read as a trap rather than a
   * checkpoint.
   */
  it("Discard rejects the plan instead of running it", async () => {
    const user = userEvent.setup();
    render(<Chat />);
    await act(async () => {
      cbs.plan.forEach((cb) =>
        cb([{ step: 1, tool: "reverb", description: "add reverb" }]),
      );
    });

    await user.click(screen.getByTestId("plan-discard-button"));

    expect(rejectPlanMock).toHaveBeenCalledTimes(1);
    expect(approvePlanMock).not.toHaveBeenCalled();
  });

  /**
   * The gate exists but was reachable only when a classifier called the
   * request a mashup, which from outside looked arbitrary. The toggle is
   * what makes it a choice.
   */
  it("the plan toggle reflects the stored preference on mount", async () => {
    getPlanFirstMock.mockResolvedValue(true);
    render(<Chat />);
    const toggle = await screen.findByTestId("plan-first-toggle");
    await act(async () => {});
    expect(toggle).toHaveAttribute("aria-pressed", "true");
  });

  it("clicking the toggle persists the new state", async () => {
    const user = userEvent.setup();
    render(<Chat />);
    const toggle = await screen.findByTestId("plan-first-toggle");
    await act(async () => {});
    expect(toggle).toHaveAttribute("aria-pressed", "false");

    await user.click(toggle);

    expect(setPlanFirstMock).toHaveBeenCalledWith(true);
    expect(toggle).toHaveAttribute("aria-pressed", "true");
  });

  /**
   * An optimistic toggle that keeps a state the backend refused would be
   * lying about what the next turn will do.
   */
  it("a failed save puts the toggle back rather than lying", async () => {
    setPlanFirstMock.mockRejectedValue(new Error("keychain locked"));
    const user = userEvent.setup();
    render(<Chat />);
    const toggle = await screen.findByTestId("plan-first-toggle");
    await act(async () => {});

    await user.click(toggle);
    await act(async () => {});

    expect(toggle).toHaveAttribute("aria-pressed", "false");
  });

});
