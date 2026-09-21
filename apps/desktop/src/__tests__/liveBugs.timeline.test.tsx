/**
 * The zoom and load-error fixes, driven through the real `Timeline`.
 *
 * These started as assertions against local copies of the arithmetic
 * and the abort predicate. Raised in review on #320: a copy proves
 * nothing about the component. Rewiring the button back to
 * `zoom ?? 50`, or deleting the abort guard outright, left those
 * versions green — which is the exact "passes for the wrong reason"
 * failure the rest of this work has been about.
 *
 * Everything below clicks the production button or rejects the
 * production `load()`.
 */

import { render, screen, waitFor, fireEvent } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

const { instances } = vi.hoisted(() => {
  const instances: {
    url: string | null;
    loadResult: () => Promise<void>;
    zoom: ReturnType<typeof vi.fn>;
  }[] = [];
  return { instances };
});

vi.mock("@tauri-apps/api/core", async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  convertFileSrc: (path: string) => `asset://${path}`,
}));

vi.mock("wavesurfer.js", () => ({
  default: {
    create: () => {
      const entry = {
        url: null as string | null,
        loadResult: () => Promise.resolve(),
        zoom: vi.fn(),
      };
      instances.push(entry);
      return {
        on: vi.fn((event: string, cb: () => void) => {
          if (event === "decode") cb();
        }),
        un: vi.fn(),
        load: vi.fn((url: string) => {
          entry.url = url;
          return entry.loadResult();
        }),
        zoom: entry.zoom,
        play: vi.fn(() => Promise.resolve()),
        pause: vi.fn(),
        seekTo: vi.fn(),
        setTime: vi.fn(),
        setVolume: vi.fn(),
        setOptions: vi.fn(),
        isPlaying: () => false,
        destroy: vi.fn(),
        // 3 seconds, matching the file that surfaced both bugs.
        getDuration: () => (entry.url ? 3 : 0),
        getCurrentTime: () => 0,
      };
    },
  },
}));

import { Timeline } from "../components/Timeline";

const TRACKS = [
  { index: 0, name: "voice", audioPath: "/tmp/voice.wav", muted: false },
];

function mount(props: Record<string, unknown> = {}) {
  instances.length = 0;
  const onZoomChange = vi.fn();
  const view = render(
    <Timeline tracks={TRACKS} onZoomChange={onZoomChange} {...props} />,
  );
  return { view, onZoomChange };
}

describe("the zoom buttons, as wired", () => {
  /**
   * The bug: `App` seeds zoom at 0 and the handler read `zoom ?? 50`,
   * which keeps 0 because `??` only catches null/undefined. `0 * 1.5`
   * is 0, so zoom-in asked for the value it already had — forever.
   */
  it("asks for a bigger zoom than it was given, from the default 0", async () => {
    const { onZoomChange } = mount({ zoom: 0 });
    await waitFor(() => expect(screen.getByTestId("zoom-in-btn")).toBeTruthy());

    fireEvent.click(screen.getByTestId("zoom-in-btn"));

    expect(onZoomChange).toHaveBeenCalledTimes(1);
    const next = onZoomChange.mock.calls[0][0] as number;
    expect(next, "zoom-in returned the value it started from").toBeGreaterThan(0);
  });

  /**
   * And it has to clear the *fitted* density, or the waveform stays
   * exactly as wide as the pane and the button still looks dead. A
   * literal 50 would satisfy the test above and fail this one.
   *
   * jsdom reports 0 for every `clientWidth`, so the fitted density is
   * not measurable here and the component falls back to 50. What is
   * asserted is the fallback's own contract; the measured path is
   * covered by the live run described in the PR.
   */
  it("uses the documented fallback when the pane cannot be measured", async () => {
    const { onZoomChange } = mount({ zoom: 0 });
    await waitFor(() => expect(screen.getByTestId("zoom-in-btn")).toBeTruthy());

    fireEvent.click(screen.getByTestId("zoom-in-btn"));
    expect(onZoomChange.mock.calls[0][0]).toBe(75);
  });

  it("steps down from an explicit level", async () => {
    const { onZoomChange } = mount({ zoom: 100 });
    await waitFor(() => expect(screen.getByTestId("zoom-out-btn")).toBeTruthy());

    fireEvent.click(screen.getByTestId("zoom-out-btn"));
    expect(onZoomChange.mock.calls[0][0]).toBe(67);
  });

  it("never asks for the level it already has", async () => {
    for (const zoom of [0, 10, 50, 100, 499]) {
      // Unmount between mounts: two Timelines in one DOM give two
      // buttons with the same test id.
      const { view, onZoomChange } = mount({ zoom });
      await waitFor(() =>
        expect(screen.getByTestId("zoom-in-btn")).toBeTruthy(),
      );
      fireEvent.click(screen.getByTestId("zoom-in-btn"));
      expect(
        onZoomChange.mock.calls[0][0],
        `zoom-in from ${zoom} returned ${zoom}`,
      ).not.toBe(zoom);
      view.unmount();
    }
  });
});

describe("a load rejection, as handled", () => {
  /**
   * An abort means *we* replaced this load. It used to land after the
   * next load had cleared the error, pinning "AbortError: Fetch is
   * aborted" under a waveform that had decoded perfectly.
   */
  it("shows nothing for an aborted load", async () => {
    instances.length = 0;
    const err = new Error("The operation was aborted.");
    err.name = "AbortError";

    const { rerender } = render(<Timeline tracks={TRACKS} />);
    await waitFor(() => expect(instances.length).toBeGreaterThan(0));

    // Every player now rejects the way an aborted fetch does.
    for (const inst of instances) {
      inst.loadResult = () => Promise.reject(err);
    }
    rerender(
      <Timeline tracks={[{ ...TRACKS[0], audioPath: "/tmp/b.wav" }]} />,
    );

    await new Promise((r) => setTimeout(r, 20));
    expect(
      screen.queryByText(/abort/i),
      "an aborted load was reported to the user",
    ).toBeNull();
  });

  /**
   * The guard must not swallow a genuine failure — that would trade
   * one silent lie for another.
   */
  it("still shows a real decode failure", async () => {
    instances.length = 0;
    const { rerender } = render(<Timeline tracks={TRACKS} />);
    await waitFor(() => expect(instances.length).toBeGreaterThan(0));

    for (const inst of instances) {
      inst.loadResult = () => Promise.reject(new Error("404 (Not Found)"));
    }
    rerender(
      <Timeline tracks={[{ ...TRACKS[0], audioPath: "/tmp/missing.wav" }]} />,
    );

    await waitFor(() =>
      expect(screen.getByText(/404 \(Not Found\)/)).toBeTruthy(),
    );
  });
});
