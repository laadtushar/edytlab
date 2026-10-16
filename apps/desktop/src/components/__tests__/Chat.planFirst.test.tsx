/**
 * The plan-first toggle starts from the stored preference, and a click
 * made before that preference arrives is not undone by it.
 *
 * The preference survives a restart (#569): it is kept in the app's
 * settings file and `get_plan_first` answers from it at launch, so the
 * toggle opens on when the person left it on. A save that fails is
 * different from a click that was refused, and the toggle shows which.
 *
 * The toggle is drawn at once, off, while `get_plan_first` is in flight.
 * A click in that gap turned it on and told the backend so; the read
 * then came back with what was stored *before* the click — off — and put
 * the toggle back. The backend kept "on", so the composer said the agent
 * would act straight away while the next turn stopped for a plan.
 *
 * Settings had exactly this race for the provider and fixed it in #324:
 * hydration is a starting point, not a correction.
 */

import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

const { getPlanFirstMock, setPlanFirstMock, unlisten } = vi.hoisted(() => ({
  getPlanFirstMock: vi.fn(),
  setPlanFirstMock: vi.fn(),
  // Chat subscribes to every agent event on mount; none fire here.
  unlisten: () => Promise.resolve(() => undefined),
}));

vi.mock("../../lib/tauri-bridge", () => ({
  sendMessage: vi.fn(),
  approvePlan: vi.fn(),
  rejectPlan: vi.fn(),
  getPlanFirst: () => getPlanFirstMock(),
  setPlanFirst: (on: boolean) => setPlanFirstMock(on),
  onTextDelta: vi.fn(unlisten),
  onToolCall: vi.fn(unlisten),
  onToolCallEnd: vi.fn(unlisten),
  onNodeCreated: vi.fn(unlisten),
  onAgentDone: vi.fn(unlisten),
  onPlan: vi.fn(unlisten),
  onPlanUnavailable: vi.fn(unlisten),
  onPlanRejected: vi.fn(unlisten),
}));

import { Chat } from "../Chat";

describe("the plan-first toggle", () => {
  beforeEach(() => {
    getPlanFirstMock.mockReset();
    setPlanFirstMock.mockReset().mockResolvedValue(undefined);
  });

  it("keeps a click made before the stored preference arrives", async () => {
    let answer!: (on: boolean) => void;
    const stored = new Promise<boolean>((resolve) => {
      answer = resolve;
    });
    getPlanFirstMock.mockReturnValue(stored);
    const user = userEvent.setup();
    render(<Chat />);

    const toggle = await screen.findByTestId("plan-first-toggle");
    await user.click(toggle);
    await waitFor(() => expect(setPlanFirstMock).toHaveBeenCalledWith(true));
    expect(toggle).toHaveAttribute("aria-pressed", "true");

    // What was stored before the click.
    await act(async () => {
      answer(false);
      await stored;
    });

    expect(toggle).toHaveAttribute("aria-pressed", "true");
  });

  it("opens on when Plan first was left on at the last quit", async () => {
    getPlanFirstMock.mockResolvedValue(true);
    render(<Chat />);
    const toggle = await screen.findByTestId("plan-first-toggle");

    await waitFor(() => expect(toggle).toHaveAttribute("aria-pressed", "true"));
    // Nothing was clicked, so nothing was sent back.
    expect(setPlanFirstMock).not.toHaveBeenCalled();
  });

  it("opens off on a fresh install", async () => {
    getPlanFirstMock.mockResolvedValue(false);
    render(<Chat />);
    const toggle = await screen.findByTestId("plan-first-toggle");

    await waitFor(() => expect(getPlanFirstMock).toHaveBeenCalled());
    expect(toggle).toHaveAttribute("aria-pressed", "false");
  });

  it("shows a choice that could not be saved as in force, and says so", async () => {
    // The backend applied it for this launch and could not write it down.
    const refusal =
      "could not save the Plan first setting (Permission denied); it applies until edytlab quits";
    getPlanFirstMock.mockResolvedValueOnce(false).mockResolvedValue(true);
    setPlanFirstMock.mockRejectedValue(refusal);
    const user = userEvent.setup();
    render(<Chat />);
    const toggle = await screen.findByTestId("plan-first-toggle");
    await waitFor(() => expect(getPlanFirstMock).toHaveBeenCalledTimes(1));

    await user.click(toggle);

    // On, because that is what the next turn will do; the toggle must not
    // promise the opposite.
    await waitFor(() => expect(getPlanFirstMock).toHaveBeenCalledTimes(2));
    expect(toggle).toHaveAttribute("aria-pressed", "true");
    expect(await screen.findByTestId("chat-error")).toHaveTextContent(refusal);
  });

  it("puts the toggle back when the click failed and the backend cannot be asked", async () => {
    getPlanFirstMock.mockResolvedValueOnce(false).mockRejectedValue("gone");
    setPlanFirstMock.mockRejectedValue("gone");
    const user = userEvent.setup();
    render(<Chat />);
    const toggle = await screen.findByTestId("plan-first-toggle");
    await waitFor(() => expect(getPlanFirstMock).toHaveBeenCalledTimes(1));

    await user.click(toggle);

    await waitFor(() => expect(getPlanFirstMock).toHaveBeenCalledTimes(2));
    await waitFor(() =>
      expect(toggle).toHaveAttribute("aria-pressed", "false"),
    );
    expect(await screen.findByTestId("chat-error")).toBeInTheDocument();
  });

  it("still starts from the stored preference when nothing was clicked", async () => {
    let answer!: (on: boolean) => void;
    const stored = new Promise<boolean>((resolve) => {
      answer = resolve;
    });
    getPlanFirstMock.mockReturnValue(stored);
    render(<Chat />);
    const toggle = await screen.findByTestId("plan-first-toggle");

    await act(async () => {
      answer(true);
      await stored;
    });

    expect(toggle).toHaveAttribute("aria-pressed", "true");
  });
});
