/**
 * Play renders the preview when there is none, then plays (#431).
 *
 * Playback plays the rendered mix, and only the Preview button rendered
 * one. So after opening a file or any edit, Space and the play button did
 * nothing: no sound, no message, and a button that was disabled with a
 * hint to go and find Preview.
 *
 * The Timeline asks for the render and plays once the mix has loaded; App
 * owns the render and the mix state. The harness below is App's wiring —
 * its `handleRenderPreview` line for line, and the real `mixIsCurrent` —
 * so what is asserted is the two halves working together. It answers the
 * render from a `held` promise so a test can act while the render is in
 * flight, which is where cancelling, failing and a head that moves on
 * all happen. The real thing, with real media playback and Space, is in
 * `e2e/play-renders-preview.spec.ts`.
 */

import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { createRef, forwardRef, useCallback, useImperativeHandle, useState } from "react";
import type { ComponentProps, RefObject } from "react";
import { describe, expect, it, vi } from "vitest";

import { held } from "./held";

type Handler = (...args: unknown[]) => void;

const { instances, loads } = vi.hoisted(() => {
  const instances: {
    url: string | null;
    currentTime: number;
    playing: boolean;
    play: ReturnType<typeof vi.fn>;
    pause: ReturnType<typeof vi.fn>;
    setTime: ReturnType<typeof vi.fn>;
  }[] = [];
  // How a player's load is answered: at once, unless a test says otherwise.
  const loads = { answer: (_url: string): Promise<void> => Promise.resolve() };
  return { instances, loads };
});

vi.mock("@tauri-apps/api/core", async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  convertFileSrc: (path: string) => `asset://${path}`,
}));

vi.mock("wavesurfer.js", () => ({
  default: {
    create: () => {
      const handlers = new Map<string, Set<Handler>>();
      const emit = (event: string, ...args: unknown[]) => {
        for (const cb of handlers.get(event) ?? []) cb(...args);
      };
      // Mirrors what the real player reports: `play` and `pause` come
      // back as events, and a new source zeroes the position.
      const entry = {
        url: null as string | null,
        currentTime: 0,
        playing: false,
        play: vi.fn(() => {
          entry.playing = true;
          emit("play");
          return Promise.resolve();
        }),
        pause: vi.fn(() => {
          entry.playing = false;
          emit("pause");
        }),
        setTime: vi.fn((t: number) => {
          entry.currentTime = t;
          emit("timeupdate", t);
        }),
      };
      instances.push(entry);
      return {
        on: vi.fn((event: string, cb: Handler) => {
          if (!handlers.has(event)) handlers.set(event, new Set());
          handlers.get(event)!.add(cb);
          if (event === "decode") cb();
        }),
        un: vi.fn((event: string, cb: Handler) => {
          handlers.get(event)?.delete(cb);
        }),
        load: vi.fn((url: string) => {
          entry.url = url;
          entry.currentTime = 0;
          entry.playing = false;
          return loads.answer(url);
        }),
        zoom: vi.fn(),
        play: entry.play,
        pause: entry.pause,
        seekTo: vi.fn(),
        setTime: entry.setTime,
        setVolume: vi.fn(),
        setOptions: vi.fn(),
        isPlaying: () => entry.playing,
        destroy: vi.fn(),
        getDuration: () => (entry.url ? 60 : 0),
        getCurrentTime: () => entry.currentTime,
        getWrapper: () => document.createElement("div"),
        setScroll: vi.fn(),
        getScroll: () => 0,
      };
    },
  },
}));

import { Timeline, type TimelineHandle } from "../components/Timeline";
import { StatusBar } from "../App";
import { mixIsCurrent } from "../lib/mixState";

const TRACKS = [{ index: 0, name: "voice", audioPath: "/tmp/voice.wav", muted: false }];

type RenderPreview = (node: string) => Promise<string>;

const previewOf = (node: string) => `/tmp/preview-${node}.wav`;
/** A render that answers when the test says (see `held`). */
const renderedBy = (answer: { promise: Promise<string> }) => vi.fn<RenderPreview>(() => answer.promise);
const holding = (path: string) => instances.find((i) => i.url === `asset://${path}`);
const playButton = () => screen.getByTestId("play-pause-button");

interface Controls {
  /** An edit lands: the head moves and the mix is cleared, as `applyNewHead` does. */
  edit(): void;
  /** Preview, pressed in the chat panel. */
  preview(): void;
}

/**
 * App's side of the contract, with the render answered by `renderPreview`.
 * `handleRenderPreview` is App's, unchanged.
 */
const Harness = forwardRef<
  Controls,
  {
    renderPreview: (node: string) => Promise<string>;
    timelineRef: RefObject<TimelineHandle | null>;
    mix?: { path: string; node: string } | null;
    tracks?: typeof TRACKS;
  }
>(function Harness({ renderPreview, timelineRef, mix = null, tracks = TRACKS }, controls) {
  const [head, setHead] = useState("node-1");
  const [mixPath, setMixPath] = useState<string | null>(mix?.path ?? null);
  const [mixNodeId, setMixNodeId] = useState<string | null>(mix?.node ?? null);
  const [rendering, setRendering] = useState(false);
  const [renderError, setRenderError] = useState<string | null>(null);

  const handleRenderPreview = useCallback(async () => {
    if (!head || rendering) return;
    setRendering(true);
    setRenderError(null);
    try {
      const path = await renderPreview(head);
      setMixPath(path);
      setMixNodeId(head);
    } catch (err) {
      setRenderError(String(err));
    } finally {
      setRendering(false);
    }
  }, [head, rendering, renderPreview]);

  useImperativeHandle(
    controls,
    () => ({
      edit: () => {
        setHead((h) => `node-${Number(h.slice(5)) + 1}`);
        setMixPath(null);
        setMixNodeId(null);
      },
      preview: () => void handleRenderPreview(),
    }),
    [handleRenderPreview],
  );

  return (
    <>
      {renderError ? <div data-testid="render-error">{renderError}</div> : null}
      <Timeline
        ref={timelineRef}
        tracks={tracks}
        mixPath={mixPath}
        mixCurrent={mixIsCurrent({ mixPath, mixNodeId }, head)}
        rendering={rendering}
        onRequestRender={handleRenderPreview}
      />
    </>
  );
});

async function mount({
  mix = null as { path: string; node: string } | null,
  renderPreview = vi.fn<RenderPreview>((node) => Promise.resolve(previewOf(node))),
  tracks = TRACKS,
} = {}) {
  instances.length = 0;
  loads.answer = () => Promise.resolve();
  const timelineRef = createRef<TimelineHandle>();
  const controls = createRef<Controls>();
  render(
    <Harness
      ref={controls}
      timelineRef={timelineRef}
      renderPreview={renderPreview}
      mix={mix}
      tracks={tracks}
    />,
  );
  if (mix) await waitFor(() => expect(holding(mix.path)).toBeDefined());
  if (mix) await waitFor(() => expect(playButton()).toBeEnabled());
  return { timelineRef, controls, renderPreview };
}

/** Let the effects and promise chains that follow an answer run out. */
async function settle() {
  await act(async () => {
    await new Promise((r) => setTimeout(r, 0));
    await new Promise((r) => setTimeout(r, 0));
  });
}

/** Every mix player that has been told to play. */
const played = () => instances.filter((i) => i.play.mock.calls.length > 0);

describe("Play with no mix", () => {
  it("is offered, and says what pressing it does", async () => {
    await mount();

    expect(playButton()).toBeEnabled();
    expect(playButton()).toHaveAccessibleName("Play");
    expect(playButton()).toHaveAttribute("title", expect.stringMatching(/renders the preview first/i));
  });

  it("renders the head once, and plays when that mix has loaded", async () => {
    const answer = held<string>();
    const { renderPreview } = await mount({ renderPreview: renderedBy(answer) });

    fireEvent.click(playButton());

    expect(renderPreview).toHaveBeenCalledTimes(1);
    expect(renderPreview).toHaveBeenCalledWith("node-1");
    // Rendering: busy, named for it, and nothing is playing yet.
    expect(playButton()).toHaveAttribute("aria-busy", "true");
    expect(playButton()).toHaveAccessibleName("Rendering…");
    expect(screen.getByTestId("play-pause-spinner")).toBeInTheDocument();
    expect(played()).toHaveLength(0);

    await answer.resolve(previewOf("node-1"));

    await waitFor(() => expect(holding(previewOf("node-1"))!.play).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(playButton()).toHaveAccessibleName("Pause"));
    expect(playButton()).not.toHaveAttribute("aria-busy", "true");
    expect(renderPreview, "one render, not one per press or per redraw").toHaveBeenCalledTimes(1);
    expect(played(), "only the mix is ever played").toHaveLength(1);
  });

  it("does not play before the mix it rendered has loaded", async () => {
    const answer = held<string>();
    const load = held<void>();
    const { renderPreview } = await mount({ renderPreview: renderedBy(answer) });
    loads.answer = (url) => (url.includes("preview-node-1") ? load.promise : Promise.resolve());

    fireEvent.click(playButton());
    await answer.resolve(previewOf("node-1"));
    await settle();

    // The render is over, the mix is on its way, and the button is still
    // waiting: a play now would be against a player with no source.
    expect(renderPreview).toHaveBeenCalledTimes(1);
    expect(played()).toHaveLength(0);
    expect(playButton()).toHaveAttribute("aria-busy", "true");

    await load.resolve();

    await waitFor(() => expect(holding(previewOf("node-1"))!.play).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(playButton()).toHaveAccessibleName("Pause"));
  });

  it("Space does the same: the toggle App's key handler calls", async () => {
    const answer = held<string>();
    const { timelineRef, renderPreview } = await mount({ renderPreview: renderedBy(answer) });

    act(() => timelineRef.current?.togglePlay());

    expect(renderPreview).toHaveBeenCalledTimes(1);
    expect(playButton()).toHaveAttribute("aria-busy", "true");

    await answer.resolve(previewOf("node-1"));

    await waitFor(() => expect(holding(previewOf("node-1"))!.play).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(playButton()).toHaveAccessibleName("Pause"));
  });

  it("a second press while it renders cancels the play, not the render", async () => {
    const answer = held<string>();
    const { renderPreview } = await mount({ renderPreview: renderedBy(answer) });

    fireEvent.click(playButton());
    expect(playButton()).toHaveAttribute("aria-busy", "true");
    fireEvent.click(playButton());

    expect(playButton()).not.toHaveAttribute("aria-busy", "true");
    expect(playButton()).toHaveAccessibleName("Play");

    // The render finishes anyway and the mix loads — silent.
    await answer.resolve(previewOf("node-1"));
    await waitFor(() => expect(holding(previewOf("node-1"))).toBeDefined());
    await settle();

    expect(played()).toHaveLength(0);
    expect(playButton()).toHaveAccessibleName("Play");
    expect(renderPreview, "cancelling is not a second render").toHaveBeenCalledTimes(1);
  });

  it("two presses before React has redrawn also cancel, rather than rendering twice", async () => {
    const answer = held<string>();
    const { timelineRef, renderPreview } = await mount({ renderPreview: renderedBy(answer) });

    act(() => {
      timelineRef.current?.togglePlay();
      timelineRef.current?.togglePlay();
    });

    expect(renderPreview).toHaveBeenCalledTimes(1);
    expect(playButton()).not.toHaveAttribute("aria-busy", "true");
    await answer.resolve(previewOf("node-1"));
    await settle();
    expect(played()).toHaveLength(0);
  });

  it("a failed render shows its error and does not play", async () => {
    const answer = held<string>();
    const { renderPreview } = await mount({ renderPreview: renderedBy(answer) });

    fireEvent.click(playButton());
    await answer.reject("render failed: out of disk space");

    expect(screen.getByTestId("render-error")).toHaveTextContent("out of disk space");
    await waitFor(() => expect(playButton()).not.toHaveAttribute("aria-busy", "true"));
    expect(playButton()).toHaveAccessibleName("Play");
    expect(played()).toHaveLength(0);

    // And the play is over, not parked for the next mix that turns up:
    // pressing again renders again.
    renderPreview.mockImplementation((node: string) => Promise.resolve(previewOf(node)));
    fireEvent.click(playButton());
    expect(renderPreview).toHaveBeenCalledTimes(2);
    await waitFor(() => expect(holding(previewOf("node-1"))!.play).toHaveBeenCalledTimes(1));
  });

  it("a mix that cannot be loaded drops the play and leaves the button usable", async () => {
    const answer = held<string>();
    await mount({ renderPreview: renderedBy(answer) });
    loads.answer = (url) =>
      url.includes("preview-node-1") ? Promise.reject(new Error("decode failed")) : Promise.resolve();

    fireEvent.click(playButton());
    await answer.resolve(previewOf("node-1"));

    expect(await screen.findByTestId("timeline-mix-error")).toHaveTextContent("decode failed");
    await waitFor(() => expect(playButton()).not.toHaveAttribute("aria-busy", "true"));
    expect(played()).toHaveLength(0);
  });

  it("is dropped when the head moves on while it renders", async () => {
    const answer = held<string>();
    const { controls } = await mount({ renderPreview: renderedBy(answer) });

    fireEvent.click(playButton());
    // An edit lands before the render does.
    act(() => controls.current?.edit());
    await answer.resolve(previewOf("node-1"));
    await settle();

    // That render is of the head the user has left, so it is not played.
    expect(played()).toHaveLength(0);
    await waitFor(() => expect(playButton()).not.toHaveAttribute("aria-busy", "true"));
    expect(playButton()).toHaveAccessibleName("Play");
  });

  it("joins a render that Preview started instead of starting another", async () => {
    const answer = held<string>();
    const { controls, renderPreview } = await mount({ renderPreview: renderedBy(answer) });

    act(() => controls.current?.preview());
    expect(renderPreview).toHaveBeenCalledTimes(1);
    fireEvent.click(playButton());
    expect(playButton()).toHaveAttribute("aria-busy", "true");

    await answer.resolve(previewOf("node-1"));

    await waitFor(() => expect(holding(previewOf("node-1"))!.play).toHaveBeenCalledTimes(1));
    expect(renderPreview).toHaveBeenCalledTimes(1);
  });
});

describe("Play with a current mix", () => {
  it("plays at once, and does not render", async () => {
    const { renderPreview } = await mount({ mix: { path: "/tmp/mix.wav", node: "node-1" } });
    const mix = holding("/tmp/mix.wav")!;

    fireEvent.click(playButton());

    expect(mix.play).toHaveBeenCalledTimes(1);
    await waitFor(() => expect(playButton()).toHaveAccessibleName("Pause"));
    expect(renderPreview).not.toHaveBeenCalled();

    // And pauses again: the toggle it always was.
    fireEvent.click(playButton());
    expect(mix.pause).toHaveBeenCalledTimes(1);
    expect(renderPreview).not.toHaveBeenCalled();
  });

  it("is what Space does too", async () => {
    const { timelineRef, renderPreview } = await mount({ mix: { path: "/tmp/mix.wav", node: "node-1" } });

    act(() => timelineRef.current?.togglePlay());

    expect(holding("/tmp/mix.wav")!.play).toHaveBeenCalledTimes(1);
    expect(renderPreview).not.toHaveBeenCalled();
  });
});

describe("Play after an edit", () => {
  it("renders the edited head, and does not play the mix of the one before", async () => {
    const { controls, renderPreview } = await mount({ mix: { path: previewOf("node-1"), node: "node-1" } });
    const before = holding(previewOf("node-1"))!;

    act(() => controls.current?.edit());
    fireEvent.click(playButton());

    expect(renderPreview).toHaveBeenCalledTimes(1);
    expect(renderPreview).toHaveBeenCalledWith("node-2");
    expect(before.play, "the old mix is another recording").not.toHaveBeenCalled();

    await waitFor(() => expect(holding(previewOf("node-2"))!.play).toHaveBeenCalledTimes(1));
    expect(before.play).not.toHaveBeenCalled();
  });

  it("plays from where the playhead was left, not from the start", async () => {
    const { controls } = await mount({ mix: { path: previewOf("node-1"), node: "node-1" } });
    holding(previewOf("node-1"))!.currentTime = 12;

    act(() => controls.current?.edit());
    fireEvent.click(playButton());

    await waitFor(() => expect(holding(previewOf("node-2"))!.play).toHaveBeenCalledTimes(1));
    const after = holding(previewOf("node-2"))!;
    expect(after.setTime).toHaveBeenCalledWith(12);
    // Placed before it starts, so it never sounds from zero.
    expect(after.setTime.mock.invocationCallOrder[0]).toBeLessThan(after.play.mock.invocationCallOrder[0]);
  });

  it("plays on a mix whose file name did not change, which reloads nothing", async () => {
    // A render is named after its node, so rendering a stale head can hand
    // back the very path that is already loaded: `mixPath` does not change,
    // nothing reloads, and "the mix has loaded" is never announced. Here the
    // loaded mix is of another node but has this node's file name.
    const { renderPreview } = await mount({
      mix: { path: previewOf("node-1"), node: "elsewhere" },
      renderPreview: vi.fn<RenderPreview>(() => Promise.resolve(previewOf("node-1"))),
    });

    fireEvent.click(playButton());

    await waitFor(() => expect(holding(previewOf("node-1"))!.play).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(playButton()).toHaveAccessibleName("Pause"));
    expect(renderPreview).toHaveBeenCalledTimes(1);
  });

  it("leaves a playing transport alone: Pause pauses, whatever mix it is on", async () => {
    const { controls, renderPreview } = await mount({ mix: { path: previewOf("node-1"), node: "node-1" } });
    const mix = holding(previewOf("node-1"))!;
    fireEvent.click(playButton());
    await waitFor(() => expect(playButton()).toHaveAccessibleName("Pause"));

    // An edit lands while it plays; its mix is stale but still sounding.
    act(() => controls.current?.edit());
    fireEvent.click(playButton());

    expect(mix.pause).toHaveBeenCalledTimes(1);
    expect(renderPreview).not.toHaveBeenCalled();
  });
});

describe("the play button when there is nothing to play", () => {
  it("is disabled with no audio, and says so", async () => {
    await mount({ tracks: [] });

    expect(playButton()).toBeDisabled();
    expect(playButton()).toHaveAttribute("title", expect.stringMatching(/open an audio file/i));
  });

  it("is disabled with audio but no way to render one", () => {
    // A Timeline that is not given a render has the transport it had
    // before: nothing to press until a mix is supplied.
    render(<Timeline tracks={TRACKS} mixPath={null} />);

    expect(playButton()).toBeDisabled();
    expect(playButton()).toHaveAttribute("title", expect.stringMatching(/render a preview first/i));
  });
});

describe("A/B compare", () => {
  it("plays the side that is chosen and never asks for a render of the head", async () => {
    instances.length = 0;
    loads.answer = () => Promise.resolve();
    const onRequestRender = vi.fn();
    // Compare puts its own renders in as the mix, and App says they are
    // current (they belong to no head).
    const view = render(
      <Timeline tracks={TRACKS} mixPath="/tmp/a.wav" mixCurrent onRequestRender={onRequestRender} />,
    );
    await waitFor(() => expect(playButton()).toBeEnabled());

    fireEvent.click(playButton());
    await waitFor(() => expect(playButton()).toHaveAccessibleName("Pause"));
    view.rerender(
      <Timeline tracks={TRACKS} mixPath="/tmp/b.wav" mixCurrent onRequestRender={onRequestRender} />,
    );

    // The crossfade carries it over: B plays on, A is the one that stops.
    await waitFor(() => expect(holding("/tmp/b.wav")!.playing).toBe(true));
    expect(playButton()).toHaveAccessibleName("Pause");
    expect(onRequestRender).not.toHaveBeenCalled();
  });
});

describe("the status bar's hint", () => {
  const bar = (props: Partial<ComponentProps<typeof StatusBar>> = {}) =>
    render(
      <StatusBar
        audioPath="/tmp/a.wav"
        head="abc1234"
        rendering={false}
        selection={null}
        mixMissing
        {...props}
      />,
    );

  it("says how to hear a session that has audio and no preview", () => {
    bar();
    expect(screen.getByTestId("status-bar-mix-missing")).toHaveTextContent(
      "preview not rendered — press Play or Preview",
    );
  });

  it("does not flash up while the preview is being rendered", () => {
    bar({ rendering: true });
    expect(screen.queryByTestId("status-bar-mix-missing")).not.toBeInTheDocument();
    expect(screen.getByTestId("status-bar")).toHaveTextContent("rendering");
  });

  it("says nothing once a mix exists, or with no audio to render", () => {
    const { rerender } = bar({ mixMissing: false });
    expect(screen.queryByTestId("status-bar-mix-missing")).not.toBeInTheDocument();
    rerender(
      <StatusBar audioPath={null} head={null} rendering={false} selection={null} mixMissing />,
    );
    expect(screen.queryByTestId("status-bar-mix-missing")).not.toBeInTheDocument();
  });

  it("gives way to a load failure, which is the bigger news", () => {
    bar({ loadError: "404 (Not Found)" });
    expect(screen.queryByTestId("status-bar-mix-missing")).not.toBeInTheDocument();
    expect(screen.getByTestId("status-bar")).toHaveTextContent("load failed");
  });
});
