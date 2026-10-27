/**
 * The bridge mock the two model rows' tests share (`SpeechModelRow.test.tsx`,
 * `SpeakerModelRow.test.tsx`): five mocks, the module that exposes them
 * under the bridge's names, and the reset every test starts from.
 *
 *   vi.mock("../lib/tauri-bridge", async () => (await import("./modelBridge")).bridgeModule());
 */

import { vi } from "vitest";

import type { MlModelStatus } from "../lib/tauri-bridge";

export const bridge = {
  mlModelStatusMock: vi.fn(),
  downloadMock: vi.fn(),
  cancelMock: vi.fn(),
  removeMock: vi.fn(),
  onProgressMock: vi.fn(),
};

/** The module the factory of `vi.mock("../lib/tauri-bridge")` returns. */
export function bridgeModule() {
  return {
    mlModelStatus: (model: string) => bridge.mlModelStatusMock(model),
    downloadMlModel: (model: string) => bridge.downloadMock(model),
    cancelMlModelDownload: (model: string) => bridge.cancelMock(model),
    removeMlModel: (model: string) => bridge.removeMock(model),
    onMlModelProgress: (cb: (s: MlModelStatus) => void) => bridge.onProgressMock(cb),
  };
}

/**
 * Resets the mocks. The row's `ml-model-progress` handler is given to
 * `onListen` when it subscribes, and `unlisten` is what the subscription
 * answers.
 */
export function resetBridge(
  onListen: (cb: (s: MlModelStatus) => void) => void,
  unlisten: () => void = () => undefined,
) {
  bridge.mlModelStatusMock.mockReset();
  bridge.downloadMock.mockReset();
  bridge.cancelMock.mockReset();
  bridge.removeMock.mockReset();
  bridge.onProgressMock.mockReset().mockImplementation((cb: (s: MlModelStatus) => void) => {
    onListen(cb);
    return Promise.resolve(unlisten);
  });
}
