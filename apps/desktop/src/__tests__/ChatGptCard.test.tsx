/**
 * The ChatGPT plan's card in Settings (#474 PR 6): each state it draws, the
 * hint that appears after a wait, what each button asks the backend for, and
 * that the pickers follow the account's own lists.
 *
 * The bridge is mocked. `emitStatus` stands in for the `chatgpt://status`
 * event, so a state is reached the way the backend reaches it.
 */

import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { ChatGptStatus } from "../lib/tauri-bridge";
import { held } from "./held";

const SIGNED_OUT: ChatGptStatus = {
  state: "signed_out",
  email: null,
  reason: null,
  tokenStorage: null,
  pendingMessage: false,
};
const SIGNED_IN: ChatGptStatus = {
  state: "signed_in",
  email: "person@example.com",
  reason: null,
  tokenStorage: { kind: "keychain", persistent: true, reason: null },
  pendingMessage: false,
};

let listener: ((s: ChatGptStatus) => void) | null = null;
const m = {
  status: vi.fn(),
  signIn: vi.fn(),
  cancel: vi.fn(),
  signOut: vi.fn(),
  forget: vi.fn(),
  usage: vi.fn(),
  listModels: vi.fn(),
  activeModel: vi.fn(),
  setModel: vi.fn(),
  getEffort: vi.fn(),
  setEffort: vi.fn(),
  confirm: vi.fn(),
};

vi.mock("../lib/tauri-bridge", () => ({
  chatgptStatus: () => m.status(),
  chatgptSignIn: (intent: string) => m.signIn(intent),
  chatgptCancelSignIn: () => m.cancel(),
  chatgptSignOut: () => m.signOut(),
  chatgptForgetRegistration: () => m.forget(),
  openChatgptUsage: () => m.usage(),
  onChatGptStatus: (cb: (s: ChatGptStatus) => void) => {
    listener = cb;
    return Promise.resolve(() => {
      listener = null;
    });
  },
  listModelsFor: (p: string) => m.listModels(p),
  getActiveModel: (p: string) => m.activeModel(p),
  setActiveModel: (p: string, id: string) => m.setModel(p, id),
  getEffortFor: (p: string) => m.getEffort(p),
  setEffortFor: (p: string, e: string | null) => m.setEffort(p, e),
}));
vi.mock("../lib/confirmAction", () => ({
  confirmAction: (q: string) => m.confirm(q),
}));

import { ChatGptCard } from "../components/ChatGptCard";

const MODELS = {
  models: [
    { id: "sol", display_name: "Sol", context_length: null, provider_hint: null, reasoning_levels: ["low", "high"] },
    { id: "luna", display_name: "Luna", context_length: null, provider_hint: null, reasoning_levels: [] },
    { id: "odd", display_name: "Odd", context_length: null, provider_hint: null, reasoning_levels: ["minimal"] },
  ],
  source: "live",
  notice: null,
};

async function emitStatus(s: ChatGptStatus | Record<string, unknown>) {
  await act(async () => {
    listener?.(s as ChatGptStatus);
  });
}

async function renderCard(status: ChatGptStatus = SIGNED_OUT, props = {}) {
  m.status.mockResolvedValue(status);
  const view = render(<ChatGptCard {...props} />);
  await waitFor(() =>
    expect(screen.getByTestId("settings-chatgpt-card")).not.toHaveAttribute("data-state", "loading"),
  );
  return view;
}

const state = () => screen.getByTestId("settings-chatgpt-card").getAttribute("data-state");

beforeEach(() => {
  listener = null;
  for (const fn of Object.values(m)) fn.mockReset();
  m.signIn.mockResolvedValue(SIGNED_IN);
  m.cancel.mockResolvedValue(undefined);
  m.usage.mockResolvedValue(undefined);
  m.listModels.mockResolvedValue(MODELS);
  m.activeModel.mockResolvedValue("sol");
  m.getEffort.mockResolvedValue(null);
  m.setModel.mockResolvedValue({ persistent: true, reason: null });
  m.setEffort.mockResolvedValue({ persistent: true, reason: null });
  m.confirm.mockResolvedValue(true);
  window.localStorage.clear();
});

afterEach(() => {
  vi.useRealTimers();
});

describe("the ChatGPT plan's card", () => {
  it("signed out: a text-only button, what it does, and who it is for", async () => {
    await renderCard();
    expect(state()).toBe("signed_out");
    const button = screen.getByRole("button", { name: "Continue with ChatGPT" });
    // Text only: no logo until OpenAI's partner team has been asked (G14).
    expect(button.querySelector("img, svg")).toBeNull();
    const card = screen.getByTestId("settings-chatgpt-card");
    expect(card).toHaveTextContent(
      "Uses your ChatGPT plan's usage instead of an API key. edytlab opens your browser to sign in.",
    );
    expect(screen.getByTestId("chatgpt-eligibility")).toHaveTextContent(
      "Available for ChatGPT Plus and Pro. May not be available on workspace plans; that depends on OpenAI and your admin.",
    );
    // Nothing is asked of the backend that signs anyone in until the click.
    expect(m.signIn).not.toHaveBeenCalled();
  });

  it("Continue signs in, and the card follows the event to Signing in and then Signed in", async () => {
    const user = userEvent.setup();
    const result = held<ChatGptStatus>();
    m.signIn.mockReturnValue(result.promise);
    const onSignedIn = vi.fn();
    await renderCard(SIGNED_OUT, { onSignedIn });

    await user.click(screen.getByTestId("chatgpt-continue"));
    expect(m.signIn).toHaveBeenCalledWith("sign_in");
    expect(state()).toBe("signing_in");
    expect(screen.getByTestId("chatgpt-status")).toHaveTextContent("Finish signing in in your browser…");

    await result.resolve(SIGNED_IN);
    await waitFor(() => expect(state()).toBe("signed_in"));
    expect(onSignedIn).toHaveBeenCalledTimes(1);
    expect(screen.getByTestId("chatgpt-account")).toHaveTextContent("Signed in as person@example.com");
    expect(screen.getByTestId("chatgpt-using-plan")).toHaveTextContent("Using ChatGPT plan");
  });

  it("Cancel while signing in asks the backend to end it", async () => {
    const user = userEvent.setup();
    await renderCard({ ...SIGNED_OUT, state: "signing_in" });
    expect(state()).toBe("signing_in");
    await user.click(screen.getByTestId("chatgpt-cancel"));
    expect(m.cancel).toHaveBeenCalledTimes(1);
  });

  it("shows the browser-error hint after about 20 seconds of waiting, and not before", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    await renderCard({ ...SIGNED_OUT, state: "signing_in" });
    expect(screen.queryByTestId("chatgpt-signing-hint")).toBeNull();

    await act(async () => {
      await vi.advanceTimersByTimeAsync(19_000);
    });
    expect(screen.queryByTestId("chatgpt-signing-hint")).toBeNull();

    await act(async () => {
      await vi.advanceTimersByTimeAsync(1_500);
    });
    expect(screen.getByTestId("chatgpt-signing-hint")).toHaveTextContent(
      "If your browser shows an error such as 'organization hasn't enabled access' or 'This app is unavailable', this account or workspace can't use ChatGPT plan sign-in. Use a personal Plus or Pro account, or an API key.",
    );

    // The wait is over: the hint goes with it, and a new wait starts at zero.
    await emitStatus(SIGNED_OUT);
    expect(screen.queryByTestId("chatgpt-signing-hint")).toBeNull();
    await emitStatus({ ...SIGNED_OUT, state: "signing_in" });
    expect(screen.queryByTestId("chatgpt-signing-hint")).toBeNull();
  });

  it("a sign-in that failed shows the sentence, and the card is not left on Signing in", async () => {
    const user = userEvent.setup();
    m.signIn.mockRejectedValue("access was denied: This app is unavailable");
    await renderCard();
    await user.click(screen.getByTestId("chatgpt-continue"));
    expect(await screen.findByTestId("chatgpt-error")).toHaveTextContent(
      "access was denied: This app is unavailable",
    );
    await waitFor(() => expect(state()).toBe("signed_out"));
  });

  it("signed in: the account, the badge, the account's models and the actions", async () => {
    await renderCard(SIGNED_IN);
    const select = await screen.findByTestId("chatgpt-model-select");
    expect(m.listModels).toHaveBeenCalledWith("chatgpt");
    expect(within(select).getAllByRole("option").map((o) => o.textContent)).toEqual([
      "Sol (sol)",
      "Luna (luna)",
      "Odd (odd)",
    ]);
    expect(select).toHaveValue("sol");
    for (const id of [
      "chatgpt-manage-usage",
      "chatgpt-sign-out",
      "chatgpt-another-account",
      "chatgpt-start-over",
    ]) {
      expect(screen.getByTestId(id)).toBeVisible();
    }
  });

  it("saves the account's first model for a person who chose none", async () => {
    m.activeModel.mockResolvedValue("");
    await renderCard(SIGNED_IN);
    await waitFor(() => expect(m.setModel).toHaveBeenCalledWith("chatgpt", "sol"));
    expect(await screen.findByTestId("chatgpt-model-select")).toHaveValue("sol");
  });

  it("saves nothing for a model it could not read, and does not guess one", async () => {
    m.activeModel.mockRejectedValue("the keychain refused");
    await renderCard(SIGNED_IN);
    const select = await screen.findByTestId("chatgpt-model-select");
    expect(select).toHaveValue("");
    expect(m.setModel).not.toHaveBeenCalled();
  });

  it("says a saved model is not in the account's list", async () => {
    m.activeModel.mockResolvedValue("gpt-retired");
    await renderCard(SIGNED_IN);
    expect(await screen.findByTestId("chatgpt-model-not-listed")).toBeVisible();
  });

  describe("the reasoning effort picker follows the model's reasoning_levels", () => {
    it("offers the levels the model lists, in the API's order, and saves the pick", async () => {
      const user = userEvent.setup();
      await renderCard(SIGNED_IN);
      const effort = await screen.findByTestId("chatgpt-effort-select");
      expect(within(effort).getAllByRole("option").map((o) => o.textContent)).toEqual([
        "Default (model's own)",
        "Low",
        "High",
      ]);
      await user.selectOptions(effort, "high");
      expect(m.setEffort).toHaveBeenCalledWith("chatgpt", "high");
      await user.selectOptions(effort, "");
      expect(m.setEffort).toHaveBeenLastCalledWith("chatgpt", null);
    });

    it("changes with the model, and is hidden for a model that lists none", async () => {
      const user = userEvent.setup();
      await renderCard(SIGNED_IN);
      await screen.findByTestId("chatgpt-effort-select");
      await user.selectOptions(screen.getByTestId("chatgpt-model-select"), "luna");
      expect(m.setModel).toHaveBeenCalledWith("chatgpt", "luna");
      expect(screen.queryByTestId("chatgpt-effort-select")).toBeNull();
    });

    it("does not offer a level this app cannot save", async () => {
      await renderCard(SIGNED_IN);
      const user = userEvent.setup();
      await screen.findByTestId("chatgpt-effort-select");
      // "minimal" is the only level `odd` lists, and the backend refuses it.
      await user.selectOptions(screen.getByTestId("chatgpt-model-select"), "odd");
      expect(screen.queryByTestId("chatgpt-effort-select")).toBeNull();
    });

    it("says when the saved level is not one the model offers", async () => {
      m.getEffort.mockResolvedValue("max");
      await renderCard(SIGNED_IN);
      expect(await screen.findByTestId("chatgpt-effort-not-offered")).toHaveTextContent("max");
      expect(screen.getByTestId("chatgpt-effort-select")).toHaveValue("");
    });
  });

  it("Manage usage asks the backend to open the page, and gives it no address", async () => {
    const user = userEvent.setup();
    await renderCard(SIGNED_IN);
    await user.click(screen.getByTestId("chatgpt-manage-usage"));
    expect(m.usage).toHaveBeenCalledTimes(1);
    expect(m.usage.mock.calls[0]).toEqual([]);
  });

  it("Sign out shows what the backend said and that it does not disconnect edytlab from ChatGPT", async () => {
    const user = userEvent.setup();
    const onSignedOut = vi.fn();
    m.signOut.mockResolvedValue({
      status: SIGNED_OUT,
      revocation: "not_confirmed",
      tokenRemoved: true,
      notice: "OpenAI could not be told to end the sign-in.",
    });
    await renderCard(SIGNED_IN, { onSignedOut });
    await user.click(screen.getByTestId("chatgpt-sign-out"));
    await waitFor(() => expect(state()).toBe("signed_out"));
    const notice = screen.getByTestId("chatgpt-end-notice");
    expect(notice).toHaveTextContent("OpenAI could not be told to end the sign-in.");
    expect(notice).toHaveTextContent("Signing out here does not disconnect edytlab from ChatGPT");
    expect(notice).toHaveTextContent("Security and login");
    expect(onSignedOut).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("button", { name: "Continue with ChatGPT" })).toBeVisible();
  });

  it("the disconnect line is there even when the backend has nothing to add", async () => {
    const user = userEvent.setup();
    m.signOut.mockResolvedValue({
      status: SIGNED_OUT,
      revocation: "confirmed",
      tokenRemoved: true,
      notice: null,
    });
    await renderCard(SIGNED_IN);
    await user.click(screen.getByTestId("chatgpt-sign-out"));
    expect(await screen.findByTestId("chatgpt-end-notice")).toHaveTextContent("does not disconnect");
  });

  it("Start over asks first, and does nothing on No", async () => {
    const user = userEvent.setup();
    m.confirm.mockResolvedValue(false);
    await renderCard(SIGNED_IN);
    await user.click(screen.getByTestId("chatgpt-start-over"));
    expect(m.confirm).toHaveBeenCalledTimes(1);
    expect(m.forget).not.toHaveBeenCalled();
    expect(state()).toBe("signed_in");
  });

  it("Start over, confirmed, forgets the registration and shows the notice", async () => {
    const user = userEvent.setup();
    m.forget.mockResolvedValue({
      status: SIGNED_OUT,
      revocation: "nothing_to_revoke",
      tokenRemoved: true,
      registrationForgotten: true,
      notice: "The old entry may still be listed in ChatGPT settings.",
    });
    await renderCard(SIGNED_IN);
    await user.click(screen.getByTestId("chatgpt-start-over"));
    await waitFor(() => expect(state()).toBe("signed_out"));
    expect(m.forget).toHaveBeenCalledTimes(1);
    expect(screen.getByTestId("chatgpt-end-notice")).toHaveTextContent("may still be listed");
    expect(screen.getByTestId("chatgpt-end-notice")).toHaveTextContent("does not disconnect");
  });

  it("Start over shows a spinner on its own button while it runs, with the state unchanged", async () => {
    const user = userEvent.setup();
    const result = held<unknown>();
    m.forget.mockReturnValue(result.promise);
    await renderCard(SIGNED_IN);
    await user.click(screen.getByTestId("chatgpt-start-over"));
    const button = screen.getByTestId("chatgpt-start-over");
    await waitFor(() => expect(within(button).getByTestId("chatgpt-spinner")).toBeInTheDocument());
    expect(button).toHaveTextContent("Starting over…");
    expect(button).toBeDisabled();
    // The backend keeps the session's own state meanwhile, never signing_in.
    expect(state()).toBe("signed_in");
    await result.resolve({
      status: SIGNED_OUT,
      revocation: "nothing_to_revoke",
      tokenRemoved: true,
      registrationForgotten: true,
      notice: null,
    });
    await waitFor(() => expect(state()).toBe("signed_out"));
    expect(screen.queryByTestId("chatgpt-spinner")).toBeNull();
  });

  it("Use another account signs in with that intent", async () => {
    const user = userEvent.setup();
    await renderCard(SIGNED_IN);
    await user.click(screen.getByTestId("chatgpt-another-account"));
    expect(m.signIn).toHaveBeenCalledWith("another_account");
  });

  it("session ended: the reason, and Continue with ChatGPT", async () => {
    const user = userEvent.setup();
    await renderCard({
      ...SIGNED_OUT,
      state: "session_ended",
      reason: { code: "refresh_token_unusable", message: "OpenAI no longer accepts the saved sign-in." },
    });
    expect(state()).toBe("session_ended");
    expect(screen.getByTestId("chatgpt-status")).toHaveTextContent("Your ChatGPT session ended.");
    expect(screen.getByTestId("chatgpt-reason")).toHaveTextContent("no longer accepts");
    await user.click(screen.getByRole("button", { name: "Continue with ChatGPT" }));
    expect(m.signIn).toHaveBeenCalledWith("sign_in");
  });

  it("plan usage declined: Turn plan usage back on runs the sign-in with enable_plan_usage", async () => {
    const user = userEvent.setup();
    await renderCard({
      ...SIGNED_IN,
      state: "plan_disabled",
      reason: { code: "plan_usage_declined", message: "Plan usage was not granted." },
    });
    expect(state()).toBe("plan_disabled");
    await user.click(screen.getByRole("button", { name: "Turn plan usage back on" }));
    expect(m.signIn).toHaveBeenCalledWith("enable_plan_usage");
  });

  it("a token that will not persist uses the keychain banner's words", async () => {
    await renderCard({
      ...SIGNED_IN,
      tokenStorage: { kind: "kernel_keyring", persistent: false, reason: "no session bus" },
    });
    const note = screen.getByTestId("chatgpt-storage-notice");
    expect(note).toHaveTextContent("No Secret Service (GNOME Keyring, KWallet or KeePassXC) is running");
    expect(note).toHaveTextContent("(no session bus)");
  });

  it("a token kept in memory says it lasts until edytlab quits", async () => {
    await renderCard({
      ...SIGNED_IN,
      tokenStorage: { kind: "memory", persistent: false, reason: null },
    });
    expect(screen.getByTestId("chatgpt-storage-notice")).toHaveTextContent("until you quit edytlab");
  });

  it("a keychain token shows no storage banner", async () => {
    await renderCard(SIGNED_IN);
    expect(screen.queryByTestId("chatgpt-storage-notice")).toBeNull();
  });

  it("unread and unavailable say so, and only unread can sign in", async () => {
    await renderCard({ ...SIGNED_OUT, state: "unread" });
    expect(state()).toBe("unread");
    expect(screen.getByTestId("chatgpt-continue")).toBeVisible();
    await emitStatus({ ...SIGNED_OUT, state: "unavailable" });
    expect(state()).toBe("unavailable");
    expect(screen.queryByTestId("chatgpt-continue")).toBeNull();
  });

  it("a state it does not know gets a neutral Working… line, not an empty card", async () => {
    await renderCard();
    await emitStatus({ ...SIGNED_OUT, state: "starting_over" });
    expect(screen.getByTestId("chatgpt-status")).toHaveTextContent("Working…");
    expect(screen.queryByTestId("chatgpt-continue")).toBeNull();
  });

  it("a status that cannot be read says so and offers to check again", async () => {
    const user = userEvent.setup();
    m.status.mockRejectedValueOnce("the backend is busy");
    render(<ChatGptCard />);
    await waitFor(() =>
      expect(screen.getByTestId("chatgpt-status")).toHaveTextContent("the backend is busy"),
    );
    m.status.mockResolvedValue(SIGNED_OUT);
    await user.click(screen.getByTestId("chatgpt-check"));
    await waitFor(() => expect(state()).toBe("signed_out"));
  });

  it("holds no token and no URL: nothing it draws looks like either", async () => {
    await renderCard(SIGNED_IN);
    const text = screen.getByTestId("settings-chatgpt-card").textContent ?? "";
    expect(text).not.toMatch(/https?:\/\//);
    expect(text).not.toMatch(/sk-|eyJ|refresh/i);
  });
});
