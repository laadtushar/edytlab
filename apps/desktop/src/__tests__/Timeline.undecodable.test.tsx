/**
 * A file that is not audio must not load silently (#322).
 *
 * Found by writing `printf 'this is not audio at all' > broken.wav`
 * and opening it in the built app: the status bar read BROKEN.WAV, the
 * lane went on drawing the previous file, the ruler kept the previous
 * file's 2.5 seconds, and nothing anywhere said a word.
 *
 * The mechanism, from wavesurfer.js 7.12.6's own source rather than
 * from guessing: `loadAudio` sets the media source and then awaits a
 * promise resolved only by `loadedmetadata`. An undecodable file makes
 * the media element fire `error` instead, so that promise never
 * settles — `load()` neither resolves nor rejects. The ticket's
 * suggested fix, checking the decode *after* `load()` resolves, would
 * never have run. `initPlayerEvents` does forward the media error to
 * WaveSurfer's own `error` event, and that is the only signal there is.
 *
 * So these tests drive `error` directly. A test that resolved `load()`
 * would be testing a path this input never takes.
 */

import { render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

const { handlers, mockLoad } = vi.hoisted(() => {
  const handlers = new Map<string, Set<(arg: unknown) => void>>();
  const mockLoad = vi.fn();
  return { handlers, mockLoad };
});

function emit(event: string, arg?: unknown) {
  for (const fn of handlers.get(event) ?? []) fn(arg);
}

vi.mock("wavesurfer.js", () => ({
  default: {
    create: () => ({
      on: (event: string, fn: (arg: unknown) => void) => {
        if (!handlers.has(event)) handlers.set(event, new Set());
        handlers.get(event)!.add(fn);
      },
      un: (event: string, fn: (arg: unknown) => void) => {
        handlers.get(event)?.delete(fn);
      },
      // The real one never settles on an undecodable file. A promise
      // that never settles is exactly that.
      load: mockLoad.mockImplementation(() => new Promise<void>(() => {})),
      zoom: vi.fn(),
      play: vi.fn(),
      pause: vi.fn(),
      seekTo: vi.fn(),
      destroy: vi.fn(),
      getDuration: () => 3,
      getCurrentTime: () => 0,
      setVolume: vi.fn(),
      setOptions: vi.fn(),
      setTime: vi.fn(),
      isPlaying: vi.fn(() => false),
      empty: vi.fn(),
    }),
  },
}));

vi.mock("@tauri-apps/api/core", async (orig) => ({
  ...(await orig<Record<string, unknown>>()),
  convertFileSrc: (p: string) => `asset://${p}`,
}));

import { Timeline } from "../components/Timeline";

describe("a file that cannot be decoded", () => {
  it("reports the failure, though load() never rejects", async () => {
    handlers.clear();
    render(<Timeline audioPath="/tmp/broken.wav" />);

    emit("error", new Error("MEDIA_ELEMENT_ERROR: Format error"));

    await waitFor(() =>
      expect(screen.getByTestId("timeline-lane-error").textContent).toMatch(
        /Format error/,
      ),
    );
  });

  it("stops drawing the previous file under the new file's name", async () => {
    handlers.clear();
    render(<Timeline audioPath="/tmp/broken.wav" />);
    // Decoded audio is on screen before the bad file arrives.
    emit("decode", 3);
    expect(screen.getByTestId("timeline-lane-waveform")).toBeVisible();

    emit("error", new Error("Format error"));

    await waitFor(() =>
      expect(screen.getByTestId("timeline-lane-waveform")).not.toBeVisible(),
    );
  });

  it("stops laying the previous file's scale under the ruler", async () => {
    // The screenshot that opened the ticket: the ruler still read
    // 0:00.0 … 0:02.5 from the previous file while the status bar had
    // already moved on to BROKEN.WAV. The lane feeds that scale, so
    // asserting on the ruler is asserting on the reported symptom.
    handlers.clear();
    render(<Timeline audioPath="/tmp/broken.wav" />);
    emit("decode", 3);

    const labels = () =>
      Array.from(screen.getByTestId("ruler").querySelectorAll("span"))
        .map((el) => el.textContent ?? "")
        .filter(Boolean);

    await waitFor(() =>
      expect(labels().some((l) => /0:0[123]/.test(l))).toBe(true),
    );

    emit("error", new Error("Format error"));

    await waitFor(() =>
      expect(labels().every((l) => /^0:00(\.0)?$/.test(l))).toBe(true),
    );
  });

  it("says nothing when an abort supersedes the load", async () => {
    // `load()` re-emits what it throws, so a cancelled load reaches the
    // same channel. Showing it would restore the permanent red box
    // that the `isAbort` guard exists to prevent.
    //
    // The abort has to be the only error, and something else has to be
    // waited for. Two mutation runs shaped this: asserting absence
    // straight after the emit passes before React has rendered at all,
    // and following the abort with a real error passes too, because
    // both land in one tick and the later message wins the box either
    // way. A decode after the abort gives an unrelated signal to wait
    // on, and an unfiltered abort would still be sitting in the box
    // when it arrives — nothing clears a load error but a new file.
    handlers.clear();
    render(<Timeline audioPath="/tmp/a.wav" />);

    emit("error", new DOMException("Fetch is aborted", "AbortError"));
    emit("decode", 3);

    await waitFor(() =>
      expect(
        Array.from(screen.getByTestId("ruler").querySelectorAll("span")).some(
          (el) => /0:0[123]/.test(el.textContent ?? ""),
        ),
      ).toBe(true),
    );
    expect(screen.queryByTestId("timeline-lane-error")).toBeNull();
  });

  it("leaves a file that decodes alone", async () => {
    handlers.clear();
    render(<Timeline audioPath="/tmp/good.wav" />);

    emit("decode", 3);

    // Wait for the decode to actually land before asserting that no
    // error came with it, so this cannot pass by running too early.
    await waitFor(() =>
      expect(
        Array.from(screen.getByTestId("ruler").querySelectorAll("span")).some(
          (el) => /0:0[123]/.test(el.textContent ?? ""),
        ),
      ).toBe(true),
    );
    expect(screen.queryByTestId("timeline-lane-error")).toBeNull();
    expect(screen.getByTestId("timeline-lane-waveform")).toBeVisible();
  });
});
