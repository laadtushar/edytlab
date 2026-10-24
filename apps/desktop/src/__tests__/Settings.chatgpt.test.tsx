/**
 * The ChatGPT plan among Settings' providers (#474 PR 6): choosing it shows
 * its card instead of a key field, activates it only when signed in (and a
 * sign-in started on the card activates it), and offers no Save or Clear key.
 */

import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

const m = {
  status: vi.fn(),
  signIn: vi.fn(),
  setProvider: vi.fn(),
  activeProvider: vi.fn(),
  baseUrl: vi.fn(),
  defaultBaseUrl: vi.fn(),
  listModels: vi.fn(),
};

vi.mock("../lib/tauri-bridge", () => ({
  chatgptStatus: () => m.status(),
  chatgptSignIn: (i: string) => m.signIn(i),
  chatgptCancelSignIn: vi.fn(),
  chatgptSignOut: vi.fn(),
  chatgptForgetRegistration: vi.fn(),
  openChatgptUsage: vi.fn(),
  onChatGptStatus: () => Promise.resolve(() => undefined),
  mlRuntimeStatus: vi.fn().mockResolvedValue({ state: "missing", searched: [] }),
  mlModelStatus: vi.fn().mockResolvedValue({ model: "whisper-base", state: "not_downloaded", bytes: 0, size: 1 }),
  onMlModelProgress: () => Promise.resolve(() => undefined),
  downloadMlModel: vi.fn(),
  cancelMlModelDownload: vi.fn(),
  removeMlModel: vi.fn(),
  getActiveProvider: () => m.activeProvider(),
  getActiveModel: vi.fn().mockResolvedValue(""),
  getBaseUrlFor: (p: string) => m.baseUrl(p),
  defaultBaseUrlFor: (p: string) => m.defaultBaseUrl(p),
  getEffortFor: vi.fn().mockResolvedValue(null),
  setEffortFor: vi.fn().mockResolvedValue({ persistent: true, reason: null }),
  listModelsFor: (p: string) => m.listModels(p),
  hasApiKeyFor: vi.fn().mockResolvedValue(true),
  clearApiKeyFor: vi.fn(),
  setApiKeyFor: vi.fn(),
  setActiveProvider: (p: string) => m.setProvider(p),
  setActiveModel: vi.fn().mockResolvedValue({ persistent: true, reason: null }),
  setBaseUrlFor: vi.fn(),
  testApiKeyFor: vi.fn(),
  installPlugin: vi.fn(),
}));

import { Settings } from "../components/Settings";

const SIGNED_OUT = { state: "signed_out", email: null, reason: null, tokenStorage: null, pendingMessage: false };
const SIGNED_IN = {
  state: "signed_in",
  email: "person@example.com",
  reason: null,
  tokenStorage: { kind: "keychain", persistent: true, reason: null },
  pendingMessage: false,
};

beforeEach(() => {
  for (const fn of Object.values(m)) fn.mockReset();
  m.status.mockResolvedValue(SIGNED_OUT);
  m.signIn.mockResolvedValue(SIGNED_IN);
  m.setProvider.mockResolvedValue({ persistent: true, reason: null });
  m.activeProvider.mockResolvedValue("anthropic");
  m.baseUrl.mockResolvedValue(null);
  m.defaultBaseUrl.mockResolvedValue("https://api.anthropic.com");
  m.listModels.mockResolvedValue({ models: [], source: "live", notice: null });
  window.localStorage.clear();
});

describe("Settings: the ChatGPT plan", () => {
  it("is offered beside the API-key providers", async () => {
    render(<Settings mode="panel" onSaved={vi.fn()} onClose={vi.fn()} />);
    expect(await screen.findByTestId("settings-provider-chatgpt")).toBeInTheDocument();
    expect(screen.getByText("ChatGPT plan")).toBeInTheDocument();
  });

  it("shows a card, not a key field, an address or Save and Clear", async () => {
    const user = userEvent.setup();
    render(<Settings mode="panel" onSaved={vi.fn()} onClose={vi.fn()} />);
    await user.click(await screen.findByTestId("settings-provider-chatgpt"));
    expect(await screen.findByTestId("settings-chatgpt-card")).toBeInTheDocument();
    for (const id of [
      "settings-key-input",
      "settings-base-url-input",
      "settings-model-input",
      "settings-test-button",
      "settings-save-button",
      "settings-clear-button",
    ]) {
      expect(screen.queryByTestId(id)).toBeNull();
    }
    // No address was read for it: the backend refuses one.
    expect(m.baseUrl).not.toHaveBeenCalledWith("chatgpt");
    expect(m.defaultBaseUrl).not.toHaveBeenCalledWith("chatgpt");
    expect(m.listModels).not.toHaveBeenCalledWith("chatgpt");
  });

  it("choosing it while signed out activates nothing", async () => {
    const user = userEvent.setup();
    const onProviderChanged = vi.fn();
    render(<Settings mode="panel" onSaved={vi.fn()} onClose={vi.fn()} onProviderChanged={onProviderChanged} />);
    await user.click(await screen.findByTestId("settings-provider-chatgpt"));
    await screen.findByTestId("chatgpt-continue");
    expect(m.setProvider).not.toHaveBeenCalled();
    expect(onProviderChanged).not.toHaveBeenCalled();
  });

  it("choosing it while signed in makes it the active provider", async () => {
    const user = userEvent.setup();
    m.status.mockResolvedValue(SIGNED_IN);
    m.listModels.mockResolvedValue({ models: [], source: "live", notice: null });
    const onProviderChanged = vi.fn();
    render(<Settings mode="panel" onSaved={vi.fn()} onClose={vi.fn()} onProviderChanged={onProviderChanged} />);
    await user.click(await screen.findByTestId("settings-provider-chatgpt"));
    await waitFor(() => expect(m.setProvider).toHaveBeenCalledWith("chatgpt"));
    expect(onProviderChanged).toHaveBeenCalledWith(true);
  });

  it("a sign-in started on the card makes it the active provider, and the app is told", async () => {
    const user = userEvent.setup();
    const onProviderChanged = vi.fn();
    render(<Settings mode="panel" onSaved={vi.fn()} onClose={vi.fn()} onProviderChanged={onProviderChanged} />);
    await user.click(await screen.findByTestId("settings-provider-chatgpt"));
    await user.click(await screen.findByTestId("chatgpt-continue"));
    await waitFor(() => expect(m.setProvider).toHaveBeenCalledWith("chatgpt"));
    expect(onProviderChanged).toHaveBeenCalledWith(true);
  });

  it("does not set the provider again when it already is the active one, which would rebuild the agent", async () => {
    const user = userEvent.setup();
    m.activeProvider.mockResolvedValue("chatgpt");
    m.status.mockResolvedValue({ ...SIGNED_OUT, state: "session_ended" });
    const onProviderChanged = vi.fn();
    render(<Settings mode="panel" onSaved={vi.fn()} onClose={vi.fn()} onProviderChanged={onProviderChanged} />);
    // The agent's provider is the plan, so Settings opens on its card.
    await user.click(await screen.findByTestId("chatgpt-continue"));
    await waitFor(() => expect(onProviderChanged).toHaveBeenCalledWith(true));
    expect(m.setProvider).not.toHaveBeenCalled();
  });

  it("a refused activation is shown", async () => {
    const user = userEvent.setup();
    m.setProvider.mockRejectedValue("sign in with ChatGPT first");
    render(<Settings mode="panel" onSaved={vi.fn()} onClose={vi.fn()} />);
    await user.click(await screen.findByTestId("settings-provider-chatgpt"));
    await user.click(await screen.findByTestId("chatgpt-continue"));
    expect(await screen.findByTestId("chatgpt-error")).toHaveTextContent("sign in with ChatGPT first");
  });

  it("signing out of the active plan tells the app it has no agent now", async () => {
    const user = userEvent.setup();
    m.activeProvider.mockResolvedValue("chatgpt");
    m.status.mockResolvedValue(SIGNED_IN);
    const onProviderChanged = vi.fn();
    render(<Settings mode="panel" onSaved={vi.fn()} onClose={vi.fn()} onProviderChanged={onProviderChanged} />);
    const signOutButton = await screen.findByTestId("chatgpt-sign-out");
    const bridge = await import("../lib/tauri-bridge");
    vi.mocked(bridge.chatgptSignOut).mockResolvedValue({
      status: SIGNED_OUT,
      revocation: "confirmed",
      tokenRemoved: true,
      notice: null,
    } as never);
    await user.click(signOutButton);
    await waitFor(() => expect(onProviderChanged).toHaveBeenCalledWith(false));
  });
});
