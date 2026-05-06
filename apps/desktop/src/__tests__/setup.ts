/**
 * Vitest setup. Runs before each test file.
 *
 * Wires `@testing-library/jest-dom` matchers and stubs the Tauri IPC
 * surface so component code that imports `@tauri-apps/api` does not
 * crash in jsdom (where there is no `window.__TAURI_INTERNALS__`).
 *
 * Tests that need to assert on bridge calls should `vi.mock` the
 * `tauri-bridge` module directly — these stubs are just no-op fallbacks
 * so an unrelated import does not blow up.
 */

import "@testing-library/jest-dom/vitest";
import { vi } from "vitest";

// @ts-expect-error - augment window for Tauri's internal IPC bridge.
window.__TAURI_INTERNALS__ = {
  invoke: vi.fn().mockResolvedValue(undefined),
  transformCallback: vi.fn(),
  ipc: vi.fn(),
};

// jsdom does not implement HTMLMediaElement playback methods used by
// wavesurfer when a track is loaded. Stub them so any incidental import
// does not throw.
if (typeof window !== "undefined" && window.HTMLMediaElement) {
  window.HTMLMediaElement.prototype.play = vi
    .fn()
    .mockResolvedValue(undefined);
  window.HTMLMediaElement.prototype.pause = vi.fn();
  window.HTMLMediaElement.prototype.load = vi.fn();
}
