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

// First, before anything that loads React — see the module.
import "./slowScheduler";
import "@testing-library/jest-dom/vitest";
import { vi } from "vitest";

// @ts-expect-error - augment window for Tauri's internal IPC bridge.
window.__TAURI_INTERNALS__ = {
  invoke: vi.fn().mockResolvedValue(undefined),
  transformCallback: vi.fn(),
  ipc: vi.fn(),
  // Missing until #322. `convertFileSrc` reads this entry directly, so
  // without it every call threw `window.__TAURI_INTERNALS__.
  // convertFileSrc is not a function` — which meant the waveform
  // lane's load path threw synchronously in *every* test that mounted
  // it, and no test had ever exercised a load that got as far as
  // starting. It went unnoticed because the only consequence was a
  // load error nothing asserted on.
  convertFileSrc: (path: string, protocol = "asset") =>
    `${protocol}://localhost/${encodeURIComponent(path)}`,
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
