/**
 * What Chat draws when a turn fails on the ChatGPT plan (#474 PR 6): the
 * usage-limit card (no retry, no fallback, the request ID, Manage usage, Copy
 * request for admin), the session-ended card and its Continue (sign in, take
 * the kept message, send it), and one sentence for the rest.
 *
 * The bridge is mocked; `sendMessage` rejects with the marker text the backend
 * writes (`plan_usage_text`).
 */

import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { held } from "./held";

const calls: string[] = [];
const m = {
  send: vi.fn(),
  signIn: vi.fn(),
  take: vi.fn(),
  usage: vi.fn(),
  provider: vi.fn(),
  status: vi.fn(),
};
let statusListener: ((s: unknown) => void) | null = null;

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
  onChatGptStatus: (cb: (s: unknown) => void) => {
    statusListener = cb;
    return Promise.resolve(() => {
      statusListener = null;
    });
  },
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

const SIGNED_IN = {
  state: "signed_in",
  email: "person@example.com",
  reason: null,
  tokenStorage: null,
  pendingMessage: false,
};

async function sendAndFail(failure: string, props: { onOpenSettings?: () => void } = {}) {
  const user = userEvent.setup();
  m.send.mockRejectedValueOnce(failure);
  render(<Chat {...props} />);
  await user.type(screen.getByLabelText("Message"), "make it louder");
  await user.click(screen.getByRole("button", { name: /send/i }));
  return user;
}

beforeEach(() => {
  calls.length = 0;
  for (const fn of Object.values(m)) fn.mockReset();
  m.send.mockResolvedValue(undefined);
  m.signIn.mockResolvedValue(SIGNED_IN);
  m.usage.mockResolvedValue(undefined);
  m.provider.mockResolvedValue("chatgpt");
  m.status.mockResolvedValue(SIGNED_IN);
  statusListener = null;
  m.take.mockResolvedValue(null);
  window.localStorage.clear();
});

describe("a usage limit on the ChatGPT plan", () => {
  const LIMIT =
    "[plan-usage code=usage_limit request-id=req_abc123] Your ChatGPT plan's usage limit has been reached.";

  it("is a card with OpenAI's three lines and a Manage usage button, not an error banner", async () => {
    await sendAndFail(LIMIT);
    const card = await screen.findByTestId("plan-usage-limit-card");
    expect(card).toHaveTextContent("Your ChatGPT plan's usage limit for apps like edytlab has been reached.");
    expect(card).toHaveTextContent(
      "An app can reach its own limit while your plan still has usage; you can change it in ChatGPT under Settings → Usage.",
    );
    expect(card).toHaveTextContent("Signing in again or retrying won't restore usage.");
    expect(screen.queryByTestId("chat-error")).toBeNull();
    expect(screen.queryByTestId("chat-error-open-settings")).toBeNull();
  });

  it("keeps the ChatGPT identity visible: the logo in the card header, beside the words", async () => {
    await sendAndFail(LIMIT);
    const card = await screen.findByTestId("plan-usage-limit-card");
    const logo = within(card).getByTestId("chatgpt-logo");
    // The card is a dark tint, so OpenAI's white file; the words say ChatGPT.
    expect(logo).toHaveAttribute("data-logo", "white");
    expect(logo).toHaveAttribute("alt", "");
    expect(logo).toHaveAttribute("aria-hidden", "true");
    expect(logo.parentElement).toHaveTextContent("ChatGPT plan's usage limit");
  });

  it("has no retry, and nothing retries or signs in by itself", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      await sendAndFail(LIMIT);
      const card = await screen.findByTestId("plan-usage-limit-card");
      expect(card.querySelector("button")).not.toBeNull();
      expect(screen.queryByRole("button", { name: /retry|try again/i })).toBeNull();
      await act(async () => {
        await vi.advanceTimersByTimeAsync(10 * 60_000);
      });
      // One send, no sign-in, no other provider asked for.
      expect(m.send).toHaveBeenCalledTimes(1);
      expect(m.signIn).not.toHaveBeenCalled();
    } finally {
      vi.useRealTimers();
    }
  });

  it("shows the request ID small, and the copy button puts it on the clipboard", async () => {
    const user = await sendAndFail(LIMIT);
    const write = vi.spyOn(navigator.clipboard, "writeText").mockResolvedValue(undefined);
    expect(await screen.findByTestId("plan-request-id")).toHaveTextContent("Request ID: req_abc123");
    await user.click(screen.getByTestId("plan-request-id-copy"));
    expect(write).toHaveBeenCalledWith("req_abc123");
    expect(await screen.findByText("Copied")).toBeVisible();
  });

  it("Copy request for admin copies a short neutral text naming edytlab and the request ID", async () => {
    const user = await sendAndFail(LIMIT);
    const write = vi.spyOn(navigator.clipboard, "writeText").mockResolvedValue(undefined);
    await user.click(await screen.findByTestId("plan-copy-for-admin"));
    expect(write).toHaveBeenCalledTimes(1);
    const text = write.mock.calls[0][0];
    expect(text).toContain("edytlab");
    expect(text).toContain("req_abc123");
    expect(text.length).toBeLessThan(300);
  });

  it("says so when the clipboard refuses", async () => {
    const user = await sendAndFail(LIMIT);
    vi.spyOn(navigator.clipboard, "writeText").mockRejectedValue(new Error("denied"));
    await user.click(await screen.findByTestId("plan-request-id-copy"));
    expect(await screen.findByText("Couldn't copy")).toBeVisible();
  });

  it("leaves the request ID out when OpenAI sent none", async () => {
    await sendAndFail("[plan-usage code=usage_limit] Your ChatGPT plan's usage limit has been reached.");
    await screen.findByTestId("plan-usage-limit-card");
    expect(screen.queryByTestId("plan-request-id")).toBeNull();
  });

  it("Manage usage asks the backend to open the page, with no address", async () => {
    const user = await sendAndFail(LIMIT);
    await user.click(await screen.findByTestId("plan-manage-usage"));
    expect(m.usage).toHaveBeenCalledTimes(1);
    expect(m.usage.mock.calls[0]).toEqual([]);
  });

  it("says why when the page could not be opened", async () => {
    m.usage.mockRejectedValue("no browser could be opened");
    const user = await sendAndFail(LIMIT);
    await user.click(await screen.findByTestId("plan-manage-usage"));
    expect(await screen.findByTestId("plan-manage-usage-error")).toHaveTextContent("no browser could be opened");
  });
});

describe("a session that ended mid-turn", () => {
  const ENDED = "[plan-usage code=session_ended request-id=req_end1] Your ChatGPT session ended.";

  it("is a card that says to continue", async () => {
    await sendAndFail(ENDED);
    expect(await screen.findByTestId("plan-session-ended-card")).toHaveTextContent(
      "Your ChatGPT session ended. Continue with ChatGPT to finish this message.",
    );
    expect(screen.queryByTestId("chat-error")).toBeNull();
  });

  it("shows the logo in its header and on the Continue with ChatGPT button, in the approved format", async () => {
    await sendAndFail(ENDED);
    const card = await screen.findByTestId("plan-session-ended-card");
    const button = within(card).getByRole("button", { name: "Continue with ChatGPT" });
    expect(button).toHaveTextContent(/^Continue with ChatGPT$/);
    // On the violet fill with dark text: the black logo.
    expect(within(button).getByTestId("chatgpt-logo")).toHaveAttribute("data-logo", "black");
    // Header logo, beside "Your ChatGPT session ended."
    const header = within(card).getAllByTestId("chatgpt-logo").find((l) => !button.contains(l));
    expect(header).toBeDefined();
    expect(header).toHaveAttribute("data-logo", "white");
    expect(header?.parentElement).toHaveTextContent("Your ChatGPT session ended.");
  });

  it("while it signs in, the button is the dark disabled one and shows no logo", async () => {
    const result = held<typeof SIGNED_IN>();
    m.signIn.mockReturnValue(result.promise);
    const user = await sendAndFail(ENDED);
    await user.click(await screen.findByTestId("plan-continue"));
    const button = await screen.findByTestId("plan-continue");
    await waitFor(() => expect(button).toBeDisabled());
    expect(within(button).queryByTestId("chatgpt-logo")).toBeNull();
    await act(async () => {
      result.resolve(SIGNED_IN);
    });
  });

  it("Continue signs in, then takes the kept message, then sends it, in that order", async () => {
    m.take.mockResolvedValue({
      text: "make it louder",
      resumeText: "make it louder",
      disabledTools: ["render_preview"],
      progressed: false,
    });
    const user = await sendAndFail(ENDED);
    await user.click(await screen.findByTestId("plan-continue"));
    await waitFor(() => expect(m.send).toHaveBeenCalledTimes(2));
    expect(calls).toEqual(["send", "sign-in", "take", "send"]);
    expect(m.signIn).toHaveBeenCalledWith("sign_in");
    expect(m.send).toHaveBeenLastCalledWith("make it louder", ["render_preview"]);
    await waitFor(() => expect(screen.queryByTestId("plan-session-ended-card")).toBeNull());
    expect(await screen.findByTestId("chat-resume-note")).toHaveTextContent("Sending your message again");
    // The person's message is in the log once.
    expect(screen.getAllByText("make it louder")).toHaveLength(1);
  });

  it("says the assistant will carry on when the turn had progressed", async () => {
    m.take.mockResolvedValue({
      text: "make it louder",
      resumeText: "Carry on from where you stopped.",
      disabledTools: [],
      progressed: true,
    });
    const user = await sendAndFail(ENDED);
    await user.click(await screen.findByTestId("plan-continue"));
    expect(await screen.findByTestId("chat-resume-note")).toHaveTextContent(
      "The assistant will carry on from where it stopped.",
    );
    expect(m.send).toHaveBeenLastCalledWith("Carry on from where you stopped.", []);
    // The carry-on line is not a message the person typed: it is not put in the log.
    expect(screen.queryByText("Carry on from where you stopped.")).toBeNull();
  });

  it("waits in the browser, and a second click cannot start a second sign-in", async () => {
    const signIn = held<typeof SIGNED_IN>();
    m.signIn.mockReturnValue(signIn.promise);
    const user = await sendAndFail(ENDED);
    await user.click(await screen.findByTestId("plan-continue"));
    expect(screen.getByTestId("plan-continue")).toBeDisabled();
    expect(screen.getByTestId("plan-continue")).toHaveTextContent("Finish signing in in your browser…");
    expect(m.signIn).toHaveBeenCalledTimes(1);
    await signIn.resolve(SIGNED_IN);
  });

  it("a cancelled sign-in keeps the card and the message, and sends nothing", async () => {
    m.signIn.mockResolvedValue({ ...SIGNED_IN, state: "signed_out" });
    const user = await sendAndFail(ENDED);
    await user.click(await screen.findByTestId("plan-continue"));
    expect(await screen.findByTestId("plan-continue-note")).toHaveTextContent("cancelled");
    expect(screen.getByTestId("plan-continue")).toBeEnabled();
    expect(m.take).not.toHaveBeenCalled();
    expect(m.send).toHaveBeenCalledTimes(1);
  });

  it("a sign-in that failed shows the sentence and keeps the message", async () => {
    m.signIn.mockRejectedValue("access was denied: This app is unavailable");
    const user = await sendAndFail(ENDED);
    await user.click(await screen.findByTestId("plan-continue"));
    expect(await screen.findByTestId("plan-continue-error")).toHaveTextContent("This app is unavailable");
    expect(m.take).not.toHaveBeenCalled();
  });

  it("nothing kept (the app was restarted) says to send the message again", async () => {
    const user = await sendAndFail(ENDED);
    await user.click(await screen.findByTestId("plan-continue"));
    expect(await screen.findByTestId("chat-resume-note")).toHaveTextContent("Send your message again");
    expect(m.send).toHaveBeenCalledTimes(1);
  });
});

describe("the other plan failures", () => {
  it.each([
    ["not_eligible", "ChatGPT plan usage isn't available for this account or workspace."],
    ["plan_disabled", "Plan usage is turned off for edytlab on this account."],
    ["unavailable", "ChatGPT isn't answering right now."],
    ["unsupported_request", "ChatGPT's plan route did not accept this request."],
    ["refused", "OpenAI refused this request."],
  ])("%s is one sentence and the request ID, never a bad API key", async (code, sentence) => {
    // OpenAI's own words can say "unauthorized", which reads as a refused key.
    await sendAndFail(`[plan-usage code=${code} request-id=req_${code}] unauthorized: invalid api key`);
    expect(await screen.findByTestId("plan-error-sentence")).toHaveTextContent(sentence);
    expect(screen.getByTestId("plan-request-id")).toHaveTextContent(`req_${code}`);
    expect(screen.queryByTestId("chat-error")).toBeNull();
    expect(screen.queryByText(/API key/i)).toBeNull();
    expect(screen.queryByTestId("chat-error-open-settings")).toBeNull();
  });

  it("a plan error card has the logo beside its sentence", async () => {
    await sendAndFail("[plan-usage code=refused] nope");
    const card = await screen.findByTestId("plan-error-card");
    const logo = within(card).getByTestId("chatgpt-logo");
    expect(logo).toHaveAttribute("data-logo", "white");
    expect(logo.parentElement).toHaveTextContent("OpenAI refused this request.");
  });

  it("unavailable offers Try again, which is a click and sends the same message", async () => {
    const user = await sendAndFail("[plan-usage code=unavailable] try later");
    await user.click(await screen.findByTestId("plan-error-retry"));
    await waitFor(() => expect(m.send).toHaveBeenCalledTimes(2));
    expect(m.send.mock.calls[1][0]).toBe("make it louder");
  });

  it("a failure that is not on the plan still reads as before", async () => {
    await sendAndFail("the model provider returned an error (401): invalid x-api-key");
    expect(await screen.findByTestId("chat-error")).toHaveTextContent("The provider rejected the API key");
    expect(screen.queryByTestId("plan-error-card")).toBeNull();
  });
});

describe("a signed-out plan with no agent", () => {
  it("is the ordinary no-agent banner, which sends the person to Settings to sign in", async () => {
    const onOpenSettings = vi.fn();
    const user = await sendAndFail(
      "no agent configured: the ChatGPT plan is not signed in. Choose Continue with ChatGPT in Settings to sign in.",
      { onOpenSettings },
    );
    expect(await screen.findByTestId("chat-error")).toHaveTextContent("Continue with ChatGPT in Settings");
    await user.click(screen.getByTestId("chat-error-open-settings"));
    expect(onOpenSettings).toHaveBeenCalledTimes(1);
    expect(screen.queryByTestId("plan-error-card")).toBeNull();
  });
});

describe("the Using ChatGPT plan line above the composer", () => {
  const line = () => screen.queryByTestId("plan-indicator");

  it("shows with the logo and Manage usage while the plan is active and signed in", async () => {
    const user = userEvent.setup();
    render(<Chat />);
    const indicator = await screen.findByTestId("plan-indicator");
    expect(indicator).toHaveTextContent("Using ChatGPT plan");
    expect(within(indicator).getByTestId("chatgpt-logo")).toHaveAttribute("data-logo", "white");
    // It is above the message box, in the composer's form.
    expect(screen.getByTestId("chat-form")).toContainElement(indicator);
    await user.click(within(indicator).getByRole("button", { name: "Manage usage" }));
    expect(m.usage).toHaveBeenCalledTimes(1);
  });

  it("is not there when another provider is active", async () => {
    m.provider.mockResolvedValue("anthropic");
    render(<Chat />);
    await waitFor(() => expect(m.provider).toHaveBeenCalled());
    expect(line()).toBeNull();
    // A person on another provider costs the ChatGPT backend nothing.
    expect(m.status).not.toHaveBeenCalled();
  });

  it("is not there when the plan is active but nobody is signed in", async () => {
    m.status.mockResolvedValue({ ...SIGNED_IN, state: "signed_out", email: null });
    render(<Chat />);
    await waitFor(() => expect(m.status).toHaveBeenCalled());
    expect(line()).toBeNull();
  });

  it("goes when the session ends or the person signs out, and comes back on a sign-in", async () => {
    render(<Chat />);
    await screen.findByTestId("plan-indicator");
    act(() => statusListener?.({ ...SIGNED_IN, state: "session_ended" }));
    await waitFor(() => expect(line()).toBeNull());
    act(() => statusListener?.(SIGNED_IN));
    expect(await screen.findByTestId("plan-indicator")).toBeInTheDocument();
  });

  it("reads the provider again when the app says it may have changed", async () => {
    m.provider.mockResolvedValue("anthropic");
    const view = render(<Chat providerEpoch={0} />);
    await waitFor(() => expect(m.provider).toHaveBeenCalledTimes(1));
    expect(line()).toBeNull();
    m.provider.mockResolvedValue("chatgpt");
    view.rerender(<Chat providerEpoch={1} />);
    expect(await screen.findByTestId("plan-indicator")).toBeInTheDocument();
  });

  it("stays out of the way when the provider cannot be read", async () => {
    m.provider.mockRejectedValue(new Error("no backend"));
    render(<Chat />);
    await waitFor(() => expect(m.provider).toHaveBeenCalled());
    expect(line()).toBeNull();
    expect(screen.getByLabelText("Message")).toBeInTheDocument();
  });
});
