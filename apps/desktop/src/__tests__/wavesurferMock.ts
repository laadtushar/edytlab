/**
 * The WaveSurfer stub the whole-`App` tests that never decode audio share
 * (`App.clipMove.test.tsx`): an instance whose methods do nothing and whose
 * duration is ten seconds.
 *
 *   vi.mock("wavesurfer.js", async () => (await import("./wavesurferMock")).wavesurferModule());
 */

import { vi } from "vitest";

/** The module the factory of `vi.mock("wavesurfer.js")` returns. */
export function wavesurferModule() {
  return {
    default: {
      create: () => ({
        on: vi.fn(),
        un: vi.fn(),
        load: vi.fn(),
        zoom: vi.fn(),
        play: vi.fn(),
        pause: vi.fn(),
        seekTo: vi.fn(),
        setTime: vi.fn(),
        setVolume: vi.fn(),
        setOptions: vi.fn(),
        isPlaying: vi.fn(() => false),
        destroy: vi.fn(),
        getDuration: () => 10,
        getCurrentTime: () => 0,
      }),
    },
  };
}
