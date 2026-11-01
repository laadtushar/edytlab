/**
 * What Chat draws when a turn fails on the ChatGPT plan (#474 PR 6): the
 * usage-limit card (no retry, no fallback, the request ID, Manage usage, Copy
 * request for admin), the session-ended card and its Continue (sign in, take
 * the kept message, send it), and one sentence for the rest.
 *
 * The bridge is mocked; `sendMessage` rejects with the marker text the backend
 * writes (`plan_usage_text`).
 */

import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

const calls: string[] = [];
const m = {
  send: vi.fn(),
  signIn: vi.fn(),
  take: vi.fn(),
  usage: vi.fn(),
  provider: vi.fn(),
  status: vi.fn(),
};

vi.mock("../lib/tauri-bridge", () => ({
  sendMessage: (text: string, disabled: string[]) => {
    calls.push("send");
    return m.send(text, disabled);
  },
  chatgptSignIn: (intent: string) => {
    calls.push("sign-in");
    return m.signIn(intent);
  },
  chatgptTakePendingMessage: () => {
    calls.push("take");
    return m.take();
  },
  openChatgptUsage: () => m.usage(),
  getActiveProvider: () => m.provider(),
  chatgptStatus: () => m.status(),
  onChatGptStatus: () => Promise.resolve(() => undefined),
  getActiveModel: vi.fn(() => Promise.resolve("")),
  setActiveModel: vi.fn(() => Promise.resolve(undefined)),
  approvePlan: vi.fn(() => Promise.resolve()),
  rejectPlan: vi.fn(() => Promise.resolve()),
  getPlanFirst: vi.fn(() => Promise.resolve(false)),
  setPlanFirst: vi.fn(() => Promise.resolve()),
  listCapabilities: vi.fn(() =>
    Promise.resolve({ tools: [], skills: [], agents: [], mcp_servers: [] }),
  ),
  onTextDelta: vi.fn(() => Promise.resolve(() => undefined)),
  onToolCall: vi.fn(() => Promise.resolve(() => undefined)),
  onToolCallEnd: vi.fn(() => Promise.resolve(() => undefined)),
  onNodeCreated: vi.fn(() => Promise.resolve(() => undefined)),
  onAgentDone: vi.fn(() => Promise.resolve(() => undefined)),
  onPlan: vi.fn(() => Promise.resolve(() => undefined)),
  onPlanUnavailable: vi.fn(() => Promise.resolve(() => undefined)),
  onPlanRejected: vi.fn(() => Promise.resolve(() => undefined)),
}));

import { Chat } from "../components/Chat";


beforeEach(() => {
  calls.length = 0;
  for (const fn of Object.values(m)) fn.mockReset();
  m.send.mockResolvedValue(undefined);
  m.provider.mockResolvedValue("chatgpt");
  m.status.mockResolvedValue({ state: "signed_in", email: null, reason: null, tokenStorage: null, pendingMessage: false });
  window.localStorage.clear();
});

async function send() {
  const user = userEvent.setup();
  const view = render(<Chat />);
  await user.type(screen.getByLabelText("Message"), "hello");
  await user.click(screen.getByRole("button", { name: /send/i }));
  return { user, view };
}

describe("the ChatGPT plan's first-use notice", () => {
  it("shows on the first send, still sends, and is gone after Got it", async () => {
    const { user } = await send();
    const dialog = await screen.findByTestId("chatgpt-first-use");
    expect(dialog).toHaveTextContent("You're using your ChatGPT plan");
    expect(m.send).toHaveBeenCalledTimes(1);
    await user.click(within(dialog).getByRole("button", { name: "Got it" }));
    await waitFor(() => expect(screen.queryByTestId("chatgpt-first-use")).toBeNull());
  });

  it("does not come back on the next send or after a relaunch", async () => {
    const first = await send();
    await first.user.click(
      within(await screen.findByTestId("chatgpt-first-use")).getByRole("button", { name: "Got it" }),
    );
    first.view.unmount();
    // A new mount stands for a relaunch: storage is what is left.
    await send();
    await waitFor(() => expect(m.send).toHaveBeenCalledTimes(2));
    expect(screen.queryByTestId("chatgpt-first-use")).toBeNull();
  });

  it("Escape dismisses it and counts as seen", async () => {
    const { user } = await send();
    const dialog = await screen.findByTestId("chatgpt-first-use");
    // Focus moves in the effect that also listens for Escape.
    await waitFor(() => expect(within(dialog).getByRole("button")).toHaveFocus());
    await user.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByTestId("chatgpt-first-use")).toBeNull());
    expect(window.localStorage.getItem("edytlab.chatgpt.firstUseSeen")).toBe("1");
  });

  it("is not shown for another provider, and is not marked seen", async () => {
    m.provider.mockResolvedValue("anthropic");
    await send();
    await waitFor(() => expect(m.send).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(m.provider).toHaveBeenCalled());
    expect(screen.queryByTestId("chatgpt-first-use")).toBeNull();
    expect(window.localStorage.getItem("edytlab.chatgpt.firstUseSeen")).toBeNull();
  });
});
