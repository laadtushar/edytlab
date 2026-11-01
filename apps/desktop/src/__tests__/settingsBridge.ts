/**
 * The parts of the bridge mock the `Settings.*.test.tsx` files share. Each
 * is a function, so every call makes fresh mocks:
 *
 *   vi.mock("../lib/tauri-bridge", async () => {
 *     const { restingAccount, speechRow } = await import("./settingsBridge");
 *     return { ...restingAccount(), ...speechRow(), mlRuntimeStatus: () => mlRuntimeStatusMock() };
 *   });
 */

import { vi } from "vitest";

/** The account commands, answering nothing: for a test that reads none of them. */
export function bareAccount() {
  return {
    getActiveProvider: vi.fn(),
    getActiveModel: vi.fn(),
    getBaseUrlFor: vi.fn(),
    defaultBaseUrlFor: vi.fn(),
    getEffortFor: vi.fn(),
    setEffortFor: vi.fn(),
    listModelsFor: vi.fn(),
    hasApiKeyFor: vi.fn(),
    clearApiKeyFor: vi.fn(),
    setApiKeyFor: vi.fn(),
    setActiveProvider: vi.fn(),
    setActiveModel: vi.fn(),
    setBaseUrlFor: vi.fn(),
    testApiKeyFor: vi.fn(),
    installPlugin: vi.fn(),
  };
}

/** The account commands, answering as an Anthropic account with a key saved would. */
export function restingAccount() {
  return {
    ...bareAccount(),
    getActiveProvider: vi.fn().mockResolvedValue(""),
    getActiveModel: vi.fn().mockResolvedValue(""),
    getBaseUrlFor: vi.fn().mockResolvedValue(null),
    defaultBaseUrlFor: vi.fn().mockResolvedValue("https://api.anthropic.com"),
    getEffortFor: vi.fn().mockResolvedValue(null),
    listModelsFor: vi.fn().mockResolvedValue({ models: [], source: "live", notice: null }),
    hasApiKeyFor: vi.fn().mockResolvedValue(true),
  };
}

/**
 * The Account tab's speech model row (#383): not downloaded, and nothing to
 * hear. Plain functions, so a test's `mockReset` leaves them alone.
 */
export function speechRow() {
  return {
    mlModelStatus: () =>
      Promise.resolve({
        model: "whisper-base",
        state: "not_downloaded",
        bytes: 0,
        size: 138_644_904,
      }),
    onMlModelProgress: () => Promise.resolve(() => undefined),
    downloadMlModel: vi.fn(),
    cancelMlModelDownload: vi.fn(),
    removeMlModel: vi.fn(),
  };
}
