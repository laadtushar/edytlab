/**
 * An audition has to be audible (#258).
 *
 * `audition_effect` rendered a real excerpt WAV and then returned only
 * its absolute path, which was printed into the chat as JSON. There was
 * no Tauri command and no player — the app's only audio sinks were the
 * timeline's source and the mix player, and neither could be reached
 * from a tool result. So the render happened, the user could not hear
 * it, and the "evaluate before you commit" workflow the tool exists for
 * was unchanged for anyone using the app.
 *
 * This drives the real listener through the bridge rather than passing
 * entries as props, so it covers the whole frontend half: the hook
 * subscribing, the view reaching the entry, and the transcript drawing
 * it. That last step is the one that matters — `Chat` renders through a
 * chain of `if (...)` guards and returns `null` at the end, so a view
 * kind nobody handles draws nothing and TypeScript stays silent. That
 * is the same silent drop one layer up, and asserting on entries would
 * miss it.
 */

import { act, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { ToolView } from "../lib/tauri-bridge";

const { cbs, noop } = vi.hoisted(() => ({
  cbs: {
    toolCall: [] as ((name: string, id: string) => void)[],
    toolCallEnd: [] as ((
      id: string,
      ok: boolean,
      view?: unknown,
    ) => void)[],
  },
  // `vi.mock` is hoisted above ordinary consts, so the stub listener
  // has to be hoisted with it.
  noop: () => Promise.resolve(() => undefined),
}));

vi.mock("../lib/tauri-bridge", () => ({
  sendMessage: vi.fn(() => Promise.resolve()),
  approvePlan: vi.fn(() => Promise.resolve()),
  rejectPlan: vi.fn(() => Promise.resolve()),
  getPlanFirst: vi.fn(() => Promise.resolve(false)),
  setPlanFirst: vi.fn(() => Promise.resolve()),
  listCapabilities: vi.fn(() =>
    Promise.resolve({ tools: [], skills: [], agents: [], mcp_servers: [] }),
  ),
  onTextDelta: vi.fn(noop),
  onToolCall: vi.fn((cb: (name: string, id: string) => void) => {
    cbs.toolCall.push(cb);
    return Promise.resolve(() => undefined);
  }),
  onToolCallEnd: vi.fn(
    (cb: (id: string, ok: boolean, view?: unknown) => void) => {
      cbs.toolCallEnd.push(cb);
      return Promise.resolve(() => undefined);
    },
  ),
  onNodeCreated: vi.fn(noop),
  onAgentDone: vi.fn(noop),
  onPlan: vi.fn(noop),
  onPlanUnavailable: vi.fn(noop),
}));

// The webview cannot open a filesystem path; Tauri's asset protocol is
// what turns one into a loadable URL. Stubbed with a recognisable
// prefix so the test can tell "converted" from "handed the raw path",
// which is the difference between audio that plays and audio that
// fails silently.
vi.mock("@tauri-apps/api/core", () => ({
  convertFileSrc: vi.fn((p: string) => `asset://localhost/${p}`),
}));

import { Chat } from "../components/Chat";

const flush = () => new Promise<void>((r) => setTimeout(r, 0));

const AUDITION: ToolView = {
  type: "audition",
  path: "/tmp/proj/.audiograph/auditions/abc123.wav",
  kind: "low_pass_filter",
  track: 1,
  start_sec: 2.5,
  end_sec: 8.5,
  summary:
    "Auditioning low_pass_filter on track 1 over 2.50s–8.50s. Nothing was added to the session.",
};

async function fire(view: ToolView | undefined) {
  render(<Chat />);
  await act(async () => {
    await flush();
  });
  expect(
    cbs.toolCall.length,
    "nothing subscribed to the tool-call event",
  ).toBeGreaterThan(0);
  await act(async () => {
    cbs.toolCall.forEach((cb) => cb("audition_effect", "call-1"));
    await flush();
  });
  await act(async () => {
    cbs.toolCallEnd.forEach((cb) => cb("call-1", true, view));
    await flush();
  });
}

describe("an audition result", () => {
  beforeEach(() => {
    cbs.toolCall = [];
    cbs.toolCallEnd = [];
  });

  it("is playable in the transcript", async () => {
    await fire(AUDITION);
    expect(screen.getByTestId("audition-player")).toBeInTheDocument();
    expect(screen.getByTestId("audition-audio")).toBeInTheDocument();
  });

  /**
   * The whole bug in one assertion. A raw path in `src` loads nothing
   * in a webview and reports no error, so this failing looks identical
   * to the player not being there at all.
   */
  it("loads the excerpt through the asset protocol, not the raw path", async () => {
    await fire(AUDITION);
    const audio = screen.getByTestId("audition-audio");
    expect(audio.getAttribute("src")).toBe(
      `asset://localhost/${AUDITION.path}`,
    );
  });

  it("has controls, or there is nothing to press", async () => {
    await fire(AUDITION);
    expect(screen.getByTestId("audition-audio")).toHaveAttribute("controls");
  });

  /** What is being auditioned, and where — otherwise two auditions in
   * a transcript are indistinguishable. */
  it("names the effect, the track and the region", async () => {
    await fire(AUDITION);
    const player = screen.getByTestId("audition-player");
    expect(player.textContent).toMatch(/low_pass_filter/);
    expect(player.textContent).toMatch(/track 1/);
    expect(player.textContent).toMatch(/2\.50s/);
    expect(player.textContent).toMatch(/8\.50s/);
  });

  /**
   * The converse, so the branch cannot pass by rendering a player for
   * every tool call. Most tool results have no view at all.
   */
  it("does not appear for a tool call with no view", async () => {
    await fire(undefined);
    expect(screen.queryByTestId("audition-player")).not.toBeInTheDocument();
  });

  /**
   * And a different view kind must not draw a player either — the
   * guard is on the tag, not on the presence of a view.
   */
  it("does not appear for a spectrum view", async () => {
    await fire({ type: "spectrum", points: [{ hz: 440, db: -6 }] });
    expect(screen.queryByTestId("audition-player")).not.toBeInTheDocument();
  });
});
