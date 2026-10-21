/**
 * A mix whose file has been evicted is rendered again, once (#607).
 *
 * The mix is an entry in the project's preview cache, which evicts its
 * least recently used entries once it is over a cap. Switching to the Graph
 * or Transcript tab unmounts the timeline, and coming back loads the file
 * again. An entry the agent's renders, auditions, graph previews or a
 * compare had evicted meanwhile answered 404, and the window showed "Could
 * not load the mix" for the mix the user was just playing. `mixNodeId` still
 * named the node shown, so nothing asked for a render.
 *
 * The backend now holds the entry the window plays, which is the cure.
 * This is the second line: a load that fails because the file is gone asks
 * for the mix again through the window's own render (`onMixMissing`), and
 * loads what that returns. One automatic try for each path: a render that
 * does not bring the file back is an error to show, not a loop. Any other
 * failure is shown as it always was.
 *
 * WaveSurfer's own message for a failed fetch is
 * `Failed to fetch <url>: <status> (<text>)` (`fetcher.js`), and Tauri's
 * asset protocol answers 404 for a file that is not there
 * (`tauri/src/protocol/asset.rs`).
 */

import { render, screen, waitFor } from "@testing-library/react";
import { useState } from "react";
import { describe, expect, it, vi } from "vitest";

const { loads } = vi.hoisted(() => {
  // How a player's load is answered, by the url it was asked for.
  const loads = {
    urls: [] as string[],
    answer: (_url: string): Promise<void> => Promise.resolve(),
  };
  return { loads };
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
          loads.urls.push(next);
          return loads.answer(next);
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
        getDuration: () => (url ? 60 : 0),
        getCurrentTime: () => 0,
        getWrapper: () => document.createElement("div"),
        setScroll: vi.fn(),
        getScroll: () => 0,
      };
    },
  },
}));

import { Timeline } from "../components/Timeline";
import { isMissingFileError } from "../lib/mixState";

const MIX = "/proj/.audiograph/previews/aaa.wav";
const MIX_URL = `asset://localhost/${MIX}`;
const TRACKS = [{ index: 0, name: "voice", audioPath: "/tmp/voice.wav", muted: false }];

/** What WaveSurfer rejects with when the asset protocol answers `status`. */
const fetchFailed = (status: number, text = "") =>
  new Error(`Failed to fetch ${MIX_URL}: ${status} (${text})`);

/** The mix player's loads of `url` so far. */
const loadsOf = (url: string) => loads.urls.filter((u) => u === url).length;

const mixError = () => screen.queryByTestId("timeline-mix-error");

/** Let the effects and promise chains that follow an answer run out. */
const settle = () => new Promise<void>((resolve) => setTimeout(resolve, 0));

function mount(onMixMissing?: () => Promise<string | null>, mixPath = MIX) {
  loads.urls.length = 0;
  return render(<Timeline tracks={TRACKS} mixPath={mixPath} onMixMissing={onMixMissing} />);
}

describe("a mix whose file is gone", () => {
  it("is rendered again once, and loaded from what that returns", async () => {
    // The first load finds nothing; the file is back after the render.
    let present = false;
    loads.answer = (url) =>
      url === MIX_URL && !present ? Promise.reject(fetchFailed(404)) : Promise.resolve();
    const onMixMissing = vi.fn(async () => {
      present = true;
      // The same file: a node renders to the same name, so `mixPath` does
      // not change and the timeline has to load it again itself.
      return MIX;
    });

    mount(onMixMissing);

    await waitFor(() => expect(loadsOf(MIX_URL)).toBe(2));
    expect(onMixMissing).toHaveBeenCalledTimes(1);
    await settle();
    expect(mixError(), "the mix loaded, so there is nothing to report").toBeNull();
  });

  it("loads the mix a render returns when it is another file", async () => {
    // The shown node moved on since the mix was made: the render is of
    // another node, `mixPath` changes, and the effect for that loads it.
    // The timeline must not also load the old path again.
    const other = "/proj/.audiograph/previews/bbb.wav";
    loads.answer = (url) => (url === MIX_URL ? Promise.reject(fetchFailed(404)) : Promise.resolve());
    let setPath: (path: string) => void = () => {};
    const onMixMissing = vi.fn(async () => {
      setPath(other);
      return other;
    });
    function Harness() {
      const [path, set] = useState(MIX);
      setPath = set;
      return <Timeline tracks={TRACKS} mixPath={path} onMixMissing={onMixMissing} />;
    }
    loads.urls.length = 0;
    render(<Harness />);

    await waitFor(() => expect(loadsOf(`asset://localhost/${other}`)).toBeGreaterThan(0));
    await settle();
    expect(loadsOf(`asset://localhost/${other}`)).toBe(1);
    expect(loadsOf(MIX_URL), "the old path was tried once, and not again").toBe(1);
    expect(onMixMissing).toHaveBeenCalledTimes(1);
    expect(mixError()).toBeNull();
  });

  it("is an error when the render does not bring the file back", async () => {
    loads.answer = (url) => (url === MIX_URL ? Promise.reject(fetchFailed(404)) : Promise.resolve());
    const onMixMissing = vi.fn(async () => MIX);

    mount(onMixMissing);

    await waitFor(() => expect(mixError()).not.toBeNull());
    expect(mixError()!.textContent).toMatch(/Could not load the mix/);
    // One try: the render, and one more load; no third.
    await settle();
    await settle();
    expect(onMixMissing, "a second render would be a loop").toHaveBeenCalledTimes(1);
    expect(loadsOf(MIX_URL)).toBe(2);
  });

  it("is an error, with no second load, when the render fails", async () => {
    loads.answer = (url) => (url === MIX_URL ? Promise.reject(fetchFailed(404)) : Promise.resolve());
    // App's render reports its own failure and answers no path.
    const onMixMissing = vi.fn(async () => null);

    mount(onMixMissing);

    await waitFor(() => expect(mixError()).not.toBeNull());
    expect(mixError()!.textContent).toMatch(/Could not load the mix/);
    await settle();
    expect(onMixMissing).toHaveBeenCalledTimes(1);
    expect(loadsOf(MIX_URL), "nothing to load again").toBe(1);
  });

  it("is an error when nothing can render it", async () => {
    // No `onMixMissing`: a caller that cannot render has what it had.
    loads.answer = () => Promise.reject(fetchFailed(404));

    mount(undefined);

    await waitFor(() => expect(mixError()).not.toBeNull());
    expect(mixError()!.textContent).toMatch(/Could not load the mix/);
    expect(loadsOf(MIX_URL)).toBe(1);
  });
});

describe("a mix that fails to load for another reason", () => {
  it.each([
    ["the asset protocol refuses the path", fetchFailed(403)],
    ["the protocol fails", fetchFailed(500, "Internal Server Error")],
    ["the request fails", new TypeError("Failed to fetch")],
    ["the file is not audio", new Error("EncodingError: Unable to decode audio data")],
  ])("is shown as an error, with no render, when %s", async (_why, failure) => {
    loads.answer = (url) => (url === MIX_URL ? Promise.reject(failure) : Promise.resolve());
    const onMixMissing = vi.fn(async () => MIX);

    mount(onMixMissing);

    await waitFor(() => expect(mixError()).not.toBeNull());
    expect(mixError()!.textContent).toMatch(/Could not load the mix/);
    await settle();
    expect(onMixMissing).not.toHaveBeenCalled();
    expect(loadsOf(MIX_URL)).toBe(1);
  });

  it("says nothing when an abort supersedes the load", async () => {
    loads.answer = (url) =>
      url === MIX_URL ? Promise.reject(new DOMException("Fetch is aborted", "AbortError")) : Promise.resolve();
    const onMixMissing = vi.fn(async () => MIX);

    mount(onMixMissing);

    await waitFor(() => expect(loadsOf(MIX_URL)).toBe(1));
    await settle();
    expect(mixError()).toBeNull();
    expect(onMixMissing).not.toHaveBeenCalled();
  });
});

describe("one try for each path", () => {
  it("allows another when the mix is another file", async () => {
    // Two different mixes lost one after the other are two losses, not one.
    const second = "/proj/.audiograph/previews/bbb.wav";
    const secondUrl = `asset://localhost/${second}`;
    loads.answer = (url) => (url === MIX_URL || url === secondUrl ? Promise.reject(fetchFailed(404)) : Promise.resolve());
    const onMixMissing = vi.fn(async () => MIX);
    loads.urls.length = 0;
    const view = render(<Timeline tracks={TRACKS} mixPath={MIX} onMixMissing={onMixMissing} />);
    await waitFor(() => expect(mixError()).not.toBeNull());
    expect(onMixMissing).toHaveBeenCalledTimes(1);

    view.rerender(<Timeline tracks={TRACKS} mixPath={second} onMixMissing={onMixMissing} />);

    await waitFor(() => expect(onMixMissing).toHaveBeenCalledTimes(2));
  });
});

describe("isMissingFileError", () => {
  it("is true for a fetch that was answered 404", () => {
    expect(isMissingFileError(fetchFailed(404))).toBe(true);
    expect(isMissingFileError(fetchFailed(404, "Not Found"))).toBe(true);
    // The message as `String(err)` makes it.
    expect(isMissingFileError(String(fetchFailed(404)))).toBe(true);
    // A Windows path, encoded as `convertFileSrc` encodes it.
    expect(
      isMissingFileError(
        new Error("Failed to fetch http://asset.localhost/C%3A%5Cproj%5Cp.wav: 404 ()"),
      ),
    ).toBe(true);
  });

  it("is false for anything else", () => {
    for (const other of [
      fetchFailed(403),
      fetchFailed(500),
      fetchFailed(416),
      new TypeError("Failed to fetch"),
      new Error("EncodingError: Unable to decode audio data"),
      // The status is in the url, not the answer.
      new Error("Failed to fetch asset://localhost/x/404.wav: 500 ()"),
      new Error("no such file 404"),
      undefined,
      null,
    ]) {
      expect(isMissingFileError(other), String(other)).toBe(false);
    }
  });
});
