/**
 * The "Spec" toggle has to change what is drawn (#254).
 *
 * It used to change only its own colour. `spectrogramEnabled` was read
 * in exactly one place — to pick the button's border — and no
 * production file imported a spectrogram plugin at all; the commit that
 * added the button ("feat(ui): spectrogram view toggle in Timeline via
 * WaveSurfer plugin") touched no plugin. Meanwhile the public changelog
 * announced "Spectrogram view toggle in the timeline" as shipped.
 *
 * The old test mocked `wavesurfer.js/dist/plugins/spectrogram.esm.js`
 * — a module nothing imported — and asserted only that the click
 * callback fired. It passed for a button wired to nothing, which is the
 * exact shape of oracle #262 was about.
 *
 * So these assert on the plugin: registered when the toggle goes on and
 * there is decoded audio, destroyed when it goes off.
 */

import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

const { created, spectrogramCreate, destroy } = vi.hoisted(() => ({
  created: [] as { registerPlugin: ReturnType<typeof vi.fn> }[],
  spectrogramCreate: vi.fn(),
  destroy: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  convertFileSrc: (p: string) => `asset://${p}`,
}));

vi.mock("wavesurfer.js", () => ({
  default: {
    create: vi.fn(() => {
      const instance = {
        // Fire `decode` immediately: the plugin reads decoded audio, so
        // the effect is guarded on a non-zero duration and would not
        // run at all against a silent mock.
        on: vi.fn((event: string, cb: () => void) => {
          if (event === "decode") cb();
        }),
        un: vi.fn(),
        load: vi.fn(() => Promise.resolve()),
        destroy: vi.fn(),
        zoom: vi.fn(),
        setVolume: vi.fn(),
        setOptions: vi.fn(),
        setTime: vi.fn(),
        registerPlugin: vi.fn((p: unknown) => p),
        getDuration: vi.fn(() => 30),
        getCurrentTime: vi.fn(() => 0),
        isPlaying: vi.fn(() => false),
      };
      created.push(instance);
      return instance;
    }),
  },
}));

vi.mock("wavesurfer.js/dist/plugins/spectrogram.esm.js", () => ({
  default: {
    create: spectrogramCreate.mockImplementation((opts: unknown) => ({
      opts,
      destroy,
    })),
  },
}));

vi.mock("wavesurfer.js/dist/plugins/timeline.esm.js", () => ({
  default: { create: vi.fn(() => ({})) },
}));

import { Timeline } from "../components/Timeline";

const TRACKS = [
  { index: 0, name: "voice", audioPath: "/tmp/voice.wav", muted: false },
];

function mount(spectrogramEnabled: boolean, onSpectrogramChange = vi.fn()) {
  created.length = 0;
  spectrogramCreate.mockClear();
  destroy.mockClear();
  const view = render(
    <Timeline
      tracks={TRACKS}
      spectrogramEnabled={spectrogramEnabled}
      onSpectrogramChange={onSpectrogramChange}
    />,
  );
  return { view, onSpectrogramChange };
}

describe("the Spec button", () => {
  it("is there, and asks for the opposite of the current state", () => {
    const { onSpectrogramChange } = mount(false);
    fireEvent.click(screen.getByTestId("spectrogram-btn"));
    expect(onSpectrogramChange).toHaveBeenCalledWith(true);
  });
});

describe("what the lane draws", () => {
  it("registers the spectrogram plugin when the toggle is on", () => {
    mount(true);

    expect(
      spectrogramCreate,
      "the lane never built a spectrogram — the toggle is decorative again",
    ).toHaveBeenCalled();

    // Registered on the lane's own WaveSurfer, not merely constructed.
    const registered = created.some((ws) =>
      ws.registerPlugin.mock.calls.length > 0,
    );
    expect(registered, "the plugin was created but never registered").toBe(true);
  });

  it("draws it over the lane's own box, so the overlays stay aligned", () => {
    mount(true);

    const host = screen.getAllByTestId("timeline-lane-spectrogram")[0];
    expect(host).toBeInTheDocument();
    expect(host.style.display).toBe("block");

    // The playhead and selection overlays are positioned against the
    // waveform box; a spectrogram of a different height would slide
    // them off the audio they point at.
    const opts = spectrogramCreate.mock.calls[0][0] as {
      container: HTMLElement;
      height: number;
    };
    expect(opts.container).toBe(host);
    expect(opts.height).toBe(72);
  });

  it("hides the waveform while the spectrogram is up", () => {
    mount(true);
    const waveform = screen.getAllByTestId("timeline-lane-waveform")[0];
    expect(waveform.style.visibility).toBe("hidden");
  });

  it("builds nothing while the toggle is off, and leaves the waveform alone", () => {
    mount(false);

    expect(spectrogramCreate).not.toHaveBeenCalled();
    const waveform = screen.getAllByTestId("timeline-lane-waveform")[0];
    expect(waveform.style.visibility).toBe("visible");
    expect(
      screen.getAllByTestId("timeline-lane-spectrogram")[0].style.display,
    ).toBe("none");
  });

  it("tears the plugin down when the toggle goes off", () => {
    const { view } = mount(true);
    expect(spectrogramCreate).toHaveBeenCalled();

    view.rerender(
      <Timeline
        tracks={TRACKS}
        spectrogramEnabled={false}
        onSpectrogramChange={vi.fn()}
      />,
    );

    expect(
      destroy,
      "the plugin outlived the toggle — it would keep drawing over the waveform",
    ).toHaveBeenCalled();
  });
});
