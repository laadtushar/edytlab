/**
 * The part of the bridge mock the `useSession` tests share
 * (`useSession.history.test.ts`, `useSession.tracks.test.tsx`): everything
 * the hook reads besides the head and the track list, which each test
 * supplies itself.
 *
 *   const bridge = vi.hoisted(() => ({ nodeCreated: [] as ((nodeId: string) => void)[], ... }));
 *   vi.mock("../lib/tauri-bridge", async () => ({
 *     getSessionHead: bridge.getSessionHead,
 *     ...(await import("./sessionBridge")).sessionBridgeCommon(bridge),
 *   }));
 *
 * `bridge.nodeCreated` holds the listeners the hook has subscribed; a test
 * that fires one reads it, and a test resets it to `[]` before each case.
 */

import { vi } from "vitest";

export function sessionBridgeCommon(bridge: { nodeCreated: ((nodeId: string) => void)[] }) {
  return {
    // Nothing saved with the project, and saving is not under test here
    // (`useSession.trail.test.ts`).
    getHeadTrail: vi.fn(() => Promise.resolve({ head: null, trail: [], redo: [] })),
    saveHeadTrail: vi.fn(() => Promise.resolve()),
    renderPreview: vi.fn(() => Promise.resolve("/tmp/mix.wav")),
    onNodeCreated: vi.fn((cb: (n: string) => void) => {
      bridge.nodeCreated.push(cb);
      return Promise.resolve(() => {
        bridge.nodeCreated = bridge.nodeCreated.filter((c) => c !== cb);
      });
    }),
    // Compactions are not under test here (`useSession.trail.test.ts`).
    onToolCall: vi.fn(() => Promise.resolve(() => undefined)),
    onToolCallEnd: vi.fn(() => Promise.resolve(() => undefined)),
    onAgentDone: vi.fn(() => Promise.resolve(() => undefined)),
    getGraph: vi.fn(() => Promise.resolve({ nodes: [], head: null })),
  };
}
