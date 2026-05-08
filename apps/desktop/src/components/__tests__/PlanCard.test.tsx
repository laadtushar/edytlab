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
  nodeCreated: [] as ((nodeId: string) => void)[],
  done: [] as (() => void)[],
  plan: [] as ((steps: Record<string, unknown>[]) => void)[],
};

const approvePlanMock = vi.fn();
const sendMessageMock = vi.fn();

vi.mock("../../lib/tauri-bridge", () => ({
  sendMessage: (text: string) => sendMessageMock(text),
  approvePlan: () => approvePlanMock(),
  onTextDelta: vi.fn((cb: (t: string) => void) => {
    cbs.textDelta.push(cb);
    return Promise.resolve(() => undefined);
  }),
  onToolCall: vi.fn((cb: (n: string, i: string) => void) => {
    cbs.toolCall.push(cb);
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
    approvePlanMock.mockResolvedValue(undefined);
    sendMessageMock.mockReset();
    sendMessageMock.mockResolvedValue(undefined);
    cbs.textDelta = [];
    cbs.toolCall = [];
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

  it("clicking Run calls approvePlan", async () => {
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
  });

  it("clicking Edit shows 'Edit coming soon' toast", async () => {
    const user = userEvent.setup();
    render(<Chat />);
    await act(async () => {
      await flush();
    });

    await act(async () => {
      cbs.plan[0](sampleSteps);
    });

    await user.click(screen.getByTestId("plan-edit-button"));
    expect(screen.getByTestId("plan-edit-toast")).toBeInTheDocument();
    expect(screen.getByTestId("plan-edit-toast").textContent).toContain(
      "Edit coming soon",
    );
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
});
