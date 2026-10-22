/**
 * The window asks for its mix again when the cache has lost the file (#607).
 *
 * The transport plays `render_preview`'s file, an entry in the project's
 * preview cache. The Graph and Transcript tabs unmount the timeline, and
 * coming back loads that file again. The backend holds the entry the window
 * plays, so the agent's renders cannot evict it; this is the second line,
 * for a file that is gone anyway. The timeline reports a load that failed
 * because the file is missing (`Timeline.mixMissing.test.tsx`), and App
 * answers it with the window's own render, `handleRenderPreview`, so what
 * is asked of the backend is `render_preview` for the node shown: once for
 * the loss, not once per load.
 *
 * The real `App` runs with only the IPC boundary and the player replaced.
 * The player answers a load by the file's state, as the asset protocol does
 * (404 for a file that is not there), and the backend's `render_preview`
 * puts the file back or not, as a test says. The real thing, in Chromium,
 * is `e2e/mix-evicted.spec.ts`.
 */

import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { afterEach, describe, expect, it, vi } from "vitest";

const { files } = vi.hoisted(() => {
  // What the asset protocol would answer for each url: nothing listed is a file.
  const files = {
    /** Urls whose file is gone, with the status the protocol answers. */
    gone: new Map<string, number>(),
    /** Every url a mix player was asked to load, in order. */
    loads: [] as string[],
  };
  return { files };
});

vi.mock("@tauri-apps/api/core", async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  convertFileSrc: (path: string) => `asset://localhost/${path}`,
}));

vi.mock("wavesurfer.js", () => ({
  default: {
    create: () => {
      let url: string | null = null;
      return {
        on: vi.fn(),
        un: vi.fn(),
        load: vi.fn((next: string) => {
          url = next;
          files.loads.push(next);
          const status = files.gone.get(next);
          return status === undefined
            ? Promise.resolve()
            : Promise.reject(new Error(`Failed to fetch ${next}: ${status} ()`));
        }),
        zoom: vi.fn(),
        play: vi.fn(() => Promise.resolve()),
        pause: vi.fn(),
        seekTo: vi.fn(),
        setTime: vi.fn(),
        setVolume: vi.fn(),
        setOptions: vi.fn(),
        isPlaying: () => false,
        destroy: vi.fn(),
        getDuration: () => (url ? 10 : 0),
        getCurrentTime: () => 0,
        getWrapper: () => document.createElement("div"),
        setScroll: vi.fn(),
        getScroll: () => 0,
      };
    },
  },
}));

import App from "../App";

type Args = Record<string, unknown>;

const HEAD = (1).toString(16).padStart(64, "0");
/** Named after the node, as the real one is. */
const MIX = `/proj/.audiograph/previews/${HEAD}.wav`;
const MIX_URL = `asset://localhost/${MIX}`;

class Backend {
  /** What a render does to the file: put it back, or leave it as it is. */
  rewrites = true;
  readonly sent: { cmd: string; args: Args }[] = [];
  calls = 0;

  handle = (cmd: string, args: Args = {}): unknown => {
    this.calls++;
    this.sent.push({ cmd, args });
    switch (cmd) {
      case "render_preview":
        if (this.rewrites) files.gone.delete(MIX_URL);
        return MIX;
      case "get_session_head":
        return HEAD;
      case "has_api_key":
        return true;
      case "get_keychain_persistence":
        return { persistent: true, reason: null };
      case "install_bundled_skills":
        return 0;
      case "list_templates":
      case "list_recent_projects":
      case "list_markers":
        return [];
      case "get_transcript":
        return { words: [], source: { kind: "unknown" } };
      case "get_view_state":
        return {};
      case "get_plan_first":
      case "get_sync_lock":
      case "get_render_warnings":
        return cmd === "get_render_warnings" ? [] : false;
      case "save_view_state":
      case "set_selection_context":
      case "save_head_trail":
        return null;
      case "get_head_trail":
        return { head: null, trail: [], redo: [] };
      case "list_tracks":
        return [
          {
            id: "3f2b8c1e-7d4a-4e9b-9c6f-1a2b3c4d5e6f",
            name: "Voice",
            muted: false,
            gain_db: 0,
            pan: 0,
            soloed: false,
            audio_path: "/tmp/voice.wav",
            clips: [
              { start_sec: 0, length_sec: 10, source_path: "/tmp/voice.wav", volume_envelope: [] },
            ],
          },
        ];
      default:
        return undefined;
    }
  };

  renders(): Args[] {
    return this.sent.filter((s) => s.cmd === "render_preview").map((s) => s.args);
  }
}

/** Let what the app does next happen. */
const settle = () =>
  act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 0));
  });

async function quiet(backend: Backend): Promise<void> {
  for (let guard = 0; guard < 20; guard++) {
    const before = backend.calls;
    await settle();
    if (backend.calls === before) return;
  }
  throw new Error("the app never stopped asking");
}

const loadsOfMix = () => files.loads.filter((u) => u === MIX_URL).length;
const mixError = () => screen.queryByTestId("timeline-mix-error");

/** Open the app, press Preview, and wait for the mix to have loaded. */
async function previewed(backend: Backend) {
  mockIPC((cmd, args) => backend.handle(cmd, args as Args | undefined), { shouldMockEvents: true });
  render(<App />);
  await screen.findByTestId("timeline-lane-mute");
  await quiet(backend);

  fireEvent.click(screen.getByTestId("render-preview-button"));
  await waitFor(() => expect(loadsOfMix()).toBeGreaterThan(0));
  await quiet(backend);
  expect(backend.renders()).toEqual([{ node: HEAD }]);
  expect(mixError()).toBeNull();
}

/** Away to the Transcript tab and back: the timeline unmounts and mounts again. */
async function switchTabsAndBack(backend: Backend) {
  fireEvent.click(screen.getByTestId("tab-transcript"));
  await waitFor(() => expect(screen.queryByTestId("timeline-lane-mute")).toBeNull());
  fireEvent.click(screen.getByTestId("tab-timeline"));
  await screen.findByTestId("timeline-lane-mute");
  await quiet(backend);
}

afterEach(() => {
  cleanup();
  clearMocks();
  files.gone.clear();
  files.loads.length = 0;
  vi.restoreAllMocks();
});

describe("a mix the cache lost while the timeline was away", () => {
  it("is rendered again on the way back, and loaded", async () => {
    const backend = new Backend();
    await previewed(backend);
    const before = loadsOfMix();
    // Evicted: the next read of it answers 404, until a render writes it.
    files.gone.set(MIX_URL, 404);

    await switchTabsAndBack(backend);

    // Waited for, not read at once: the load that finds the file gone and
    // the render it asks for are answers that React draws on its own schedule.
    await waitFor(() =>
      expect(backend.renders(), "the loss is one render of the node shown").toEqual([
        { node: HEAD },
        { node: HEAD },
      ]),
    );
    await waitFor(() =>
      expect(loadsOfMix() - before, "the lost load, and the load of what was rendered").toBe(2),
    );
    await quiet(backend);
    expect(mixError()).toBeNull();
    expect(backend.renders()).toHaveLength(2);
  });

  it("is an error after one try when the render does not bring the file back", async () => {
    const backend = new Backend();
    await previewed(backend);
    backend.rewrites = false;
    files.gone.set(MIX_URL, 404);

    await switchTabsAndBack(backend);
    await quiet(backend);

    await waitFor(() => expect(mixError()).not.toBeNull());
    expect(mixError()!.textContent).toMatch(/Could not load the mix/);
    expect(backend.renders(), "one try, and no loop").toHaveLength(2);
  });

  it("is not rendered again for a failure that is not a lost file", async () => {
    const backend = new Backend();
    await previewed(backend);
    files.gone.set(MIX_URL, 403);

    await switchTabsAndBack(backend);

    await waitFor(() => expect(mixError()).not.toBeNull());
    expect(mixError()!.textContent).toMatch(/Could not load the mix/);
    expect(backend.renders(), "a refused path is not repaired by rendering").toEqual([{ node: HEAD }]);
  });

  it("is not rendered again while the file is there", async () => {
    const backend = new Backend();
    await previewed(backend);
    const before = loadsOfMix();

    await switchTabsAndBack(backend);

    await waitFor(() =>
      expect(loadsOfMix() - before, "the mix is read again on the way back").toBe(1),
    );
    await quiet(backend);
    expect(backend.renders()).toEqual([{ node: HEAD }]);
    expect(mixError()).toBeNull();
  });
});
