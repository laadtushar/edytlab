/**
 * A skipped plan gate has to be visible (#267).
 *
 * `fetch_plan` collapsed every transport and HTTP failure into `None`,
 * and the caller falls through to the tool loop on `None`. A user who
 * turned Plan First on lost the checkpoint they asked for, and it looked
 * exactly like the model deciding no plan was needed — no event existed
 * that could tell the two apart, and nothing was logged either.
 *
 * This drives the real listener through the bridge, so it covers the
 * whole frontend half: the hook subscribing, the entry reaching the
 * transcript, and the transcript drawing it. That last step is not
 * incidental — `Chat` renders through a chain of `if (isX(entry))`
 * guards and returns `null` at the end, so an entry kind nobody handles
 * draws nothing and TypeScript does not complain. Asserting on the
 * entry list would have missed exactly that.
 */

import { act, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const { cbs, noop } = vi.hoisted(() => ({
  cbs: { planUnavailable: [] as ((reason: string) => void)[] },
  // `vi.mock` is hoisted above ordinary consts, so the stub listener has
  // to be hoisted with it.
  noop: () => Promise.resolve(() => undefined),
}));

vi.mock("../lib/tauri-bridge", () => ({
  sendMessage: vi.fn(() => Promise.resolve()),
  approvePlan: vi.fn(() => Promise.resolve()),
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
  onPlan: vi.fn(noop),
  onPlanUnavailable: vi.fn((cb: (reason: string) => void) => {
    cbs.planUnavailable.push(cb);
    return Promise.resolve(() => undefined);
  }),
}));

import { Chat } from "../components/Chat";

const flush = () => new Promise<void>((r) => setTimeout(r, 0));

async function mountAndFire(reason: string) {
  render(<Chat />);
  await act(async () => {
    await flush();
  });
  expect(
    cbs.planUnavailable.length,
    "nothing subscribed to the plan-unavailable event",
  ).toBeGreaterThan(0);
  await act(async () => {
    cbs.planUnavailable.forEach((cb) => cb(reason));
    await flush();
  });
}

describe("a skipped plan gate", () => {
  beforeEach(() => {
    cbs.planUnavailable = [];
  });

  it("appears in the transcript rather than being dropped", async () => {
    await mountAndFire("the planning request returned HTTP 503");
    expect(screen.getByTestId("chat-notice")).toBeInTheDocument();
  });

  /**
   * The reason is the difference between "the service hiccupped" and
   * "the model chose not to plan". Without it the user is exactly as
   * informed as they were before.
   */
  it("names the failure", async () => {
    await mountAndFire("the planning request returned HTTP 503");
    expect(screen.getByTestId("chat-notice").textContent).toMatch(/HTTP 503/);
  });

  it("says the turn is going ahead without the gate", async () => {
    await mountAndFire("the planning request failed: connection reset");
    const text = screen.getByTestId("chat-notice").textContent ?? "";
    expect(text).toMatch(/skipped/i);
    expect(text).toMatch(/continuing without/i);
  });

  it("draws nothing until the event arrives", async () => {
    render(<Chat />);
    await act(async () => {
      await flush();
    });
    expect(screen.queryByTestId("chat-notice")).not.toBeInTheDocument();
  });
});
