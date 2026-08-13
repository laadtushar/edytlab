/**
 * Clip strip (#103).
 *
 * The ticket's stated risk is gesture collision: the lane surface
 * already owns range selection and loop dragging, and getting the
 * arbitration wrong means a range drag starts moving audio. The strip
 * sidesteps that by being its own row rather than an overlay, so the
 * last test here checks the thing that would actually break — that the
 * waveform's own selection still works with clips on screen.
 */

import { fireEvent, render, screen } from "@testing-library/react";
import type { MockedFunction } from "vitest";
import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("wavesurfer.js", () => ({
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
      isPlaying: vi.fn(() => false),
      destroy: vi.fn(),
      getDuration: () => 10,
      getCurrentTime: () => 0,
    }),
  },
}));

import { ClipStrip, clipLabel } from "../components/ClipStrip";
import { Timeline } from "../components/Timeline";
import type { ClipSummary } from "../lib/tauri-bridge";

/** A track cut in two, with a silent gap between the halves. */
const TWO_CLIPS: ClipSummary[] = [
  {
    start_sec: 0,
    length_sec: 2,
    source_path: "/tmp/take.wav",
    volume_envelope: [],
  },
  {
    start_sec: 5,
    length_sec: 3,
    source_path: "/tmp/take.wav",
    volume_envelope: [],
  },
];

function stubStripWidth(width = 1000) {
  const strip = screen.getByRole("group", { name: /clips/ });
  strip.getBoundingClientRect = () =>
    ({ left: 0, top: 0, width, height: 22, right: width, bottom: 22 }) as DOMRect;
}

describe("clipLabel", () => {
  it("uses the file name", () => {
    expect(clipLabel(TWO_CLIPS[0], 0)).toBe("take.wav");
  });

  it("falls back to a position when there is no path", () => {
    expect(
      clipLabel({ ...TWO_CLIPS[0], source_path: "" }, 2),
    ).toBe("clip 3");
  });
});

describe("ClipStrip", () => {
  type Move = (clipIndex: number, startSec: number) => void;
  let onMoveClip: MockedFunction<Move>;
  let onSelectClip: MockedFunction<(i: number | null) => void>;
  let onRemoveClip: MockedFunction<(i: number) => void>;

  beforeEach(() => {
    onMoveClip = vi.fn<Move>();
    onSelectClip = vi.fn<(i: number | null) => void>();
    onRemoveClip = vi.fn<(i: number) => void>();
  });

  function renderStrip(selected: number | null = null) {
    return render(
      <ClipStrip
        clips={TWO_CLIPS}
        duration={10}
        trackName="take"
        selectedClip={selected}
        onSelectClip={onSelectClip}
        onMoveClip={onMoveClip}
        onRemoveClip={onRemoveClip}
      />,
    );
  }

  it("draws one chip per clip, positioned by start and length", () => {
    renderStrip();
    // The seam is the whole point: a split track showed one continuous
    // lane before this, so two chips with a gap is the fix.
    const first = screen.getByTestId("clip-chip-0");
    const second = screen.getByTestId("clip-chip-1");
    expect(first.style.left).toBe("0%");
    expect(first.style.width).toBe("20%");
    expect(second.style.left).toBe("50%");
    expect(second.style.width).toBe("30%");
  });

  it("selects on press", () => {
    renderStrip();
    fireEvent.pointerDown(screen.getByTestId("clip-chip-1"), { clientX: 500 });
    expect(onSelectClip).toHaveBeenCalledWith(1);
  });

  it("shows which clip is selected", () => {
    renderStrip(1);
    expect(screen.getByTestId("clip-chip-1")).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    expect(screen.getByTestId("clip-chip-0")).toHaveAttribute(
      "aria-pressed",
      "false",
    );
  });

  it("does not write a node for a click that never became a drag", () => {
    renderStrip();
    stubStripWidth();
    const chip = screen.getByTestId("clip-chip-0");
    fireEvent.pointerDown(chip, { clientX: 100 });
    // Within DRAG_SLOP — a click, not a move.
    fireEvent.pointerMove(window, { clientX: 101 });
    fireEvent.pointerUp(window, { clientX: 101 });
    expect(onMoveClip).not.toHaveBeenCalled();
    expect(onSelectClip).toHaveBeenCalledWith(0);
  });

  it("writes once on release, not on every pointer move", () => {
    renderStrip();
    stubStripWidth();
    fireEvent.pointerDown(screen.getByTestId("clip-chip-0"), { clientX: 100 });
    for (const x of [200, 300, 400]) {
      fireEvent.pointerMove(window, { clientX: x });
    }
    expect(onMoveClip).not.toHaveBeenCalled();

    fireEvent.pointerUp(window, { clientX: 400 });
    expect(onMoveClip).toHaveBeenCalledTimes(1);
    // 300 px of 1000 across a 10 s timeline = +3 s, from a start of 0.
    expect(onMoveClip).toHaveBeenCalledWith(0, 3);
  });

  it("clamps a drag before the start of the timeline", () => {
    renderStrip();
    stubStripWidth();
    fireEvent.pointerDown(screen.getByTestId("clip-chip-0"), { clientX: 500 });
    fireEvent.pointerUp(window, { clientX: 0 });
    expect(onMoveClip).toHaveBeenCalledWith(0, 0);
  });

  it("is movable and removable from the keyboard", () => {
    renderStrip();
    const chip = screen.getByTestId("clip-chip-1");
    chip.focus();
    expect(chip).toHaveFocus();
    expect(chip).toHaveAccessibleName("take.wav, 5.00 to 8.00 seconds");

    fireEvent.keyDown(chip, { key: "ArrowRight" });
    expect(onMoveClip).toHaveBeenLastCalledWith(1, 5.1);

    fireEvent.keyDown(chip, { key: "Delete" });
    expect(onRemoveClip).toHaveBeenCalledWith(1);
  });

  it("reconciles from the session rather than keeping its draft", () => {
    const { rerender } = renderStrip();
    stubStripWidth();
    fireEvent.pointerDown(screen.getByTestId("clip-chip-0"), { clientX: 100 });
    fireEvent.pointerMove(window, { clientX: 400 });
    expect(screen.getByTestId("clip-chip-0").style.left).toBe("30%");
    fireEvent.pointerUp(window, { clientX: 400 });

    // The refresh says the move did not take — e.g. it was rejected.
    rerender(
      <ClipStrip
        clips={TWO_CLIPS.map((c) => ({ ...c }))}
        duration={10}
        trackName="take"
        selectedClip={null}
        onSelectClip={onSelectClip}
        onMoveClip={onMoveClip}
        onRemoveClip={onRemoveClip}
      />,
    );
    expect(screen.getByTestId("clip-chip-0").style.left).toBe("0%");
  });
});

describe("Timeline integration", () => {
  const TRACKS = [
    {
      index: 0,
      name: "take",
      audioPath: "/tmp/take.wav",
      muted: false,
      gainDb: 0,
      pan: 0,
      soloed: false,
      clips: TWO_CLIPS,
    },
  ];

  it("hides the strip when no clip handlers are supplied", () => {
    render(<Timeline tracks={TRACKS} audioPath="/tmp/take.wav" />);
    expect(screen.queryByTestId("clip-strip")).not.toBeInTheDocument();
  });

  it("shows the strip once clip handlers are supplied", () => {
    render(
      <Timeline
        tracks={TRACKS}
        audioPath="/tmp/take.wav"
        onMoveClip={vi.fn()}
        onRemoveClip={vi.fn()}
      />,
    );
    expect(screen.getByTestId("clip-strip")).toBeInTheDocument();
    expect(screen.getByTestId("clip-chip-1")).toBeInTheDocument();
  });

  it("addresses clips by session track index, not lane index", () => {
    const onMoveClip = vi.fn();
    render(
      <Timeline
        // Lane 0, but session track 3 — App filters out tracks with no
        // audio, so the two can differ.
        tracks={[{ ...TRACKS[0], index: 3 }]}
        audioPath="/tmp/take.wav"
        onMoveClip={onMoveClip}
        onRemoveClip={vi.fn()}
      />,
    );
    const strip = screen.getByRole("group", { name: /clips/ });
    strip.getBoundingClientRect = () =>
      ({ left: 0, top: 0, width: 1000, height: 22 }) as DOMRect;
    fireEvent.pointerDown(screen.getByTestId("clip-chip-0"), { clientX: 100 });
    fireEvent.pointerUp(window, { clientX: 400 });
    expect(onMoveClip).toHaveBeenCalledWith(3, 0, expect.any(Number));
  });

  /**
   * The regression the ticket calls out by name. The strip is its own
   * row rather than an overlay precisely so this cannot break, and this
   * asserts it rather than trusting the layout.
   */
  it("leaves the waveform's own gestures alone", () => {
    const onMoveClip = vi.fn();
    render(
      <Timeline
        tracks={TRACKS}
        audioPath="/tmp/take.wav"
        onSelectionChange={vi.fn()}
        onMoveClip={onMoveClip}
        onRemoveClip={vi.fn()}
      />,
    );
    const strip = screen.getByTestId("clip-strip");
    const lane = screen.getAllByTestId("timeline-lane-sidebar")[0]
      .parentElement as HTMLElement;

    // Structural: the strip is a sibling row, not an overlay. This is
    // the mechanism that makes the collision impossible rather than
    // merely unlikely.
    expect(strip.contains(lane)).toBe(false);
    expect(lane.contains(strip)).toBe(false);

    // Behavioural: a press and drag anywhere on the waveform lane must
    // not reach the strip's move handler. Get this wrong and selecting
    // a range starts moving audio, which is the regression #103 warns
    // about by name.
    fireEvent.pointerDown(lane, { clientX: 100 });
    fireEvent.pointerMove(window, { clientX: 400 });
    fireEvent.pointerUp(window, { clientX: 400 });
    expect(onMoveClip).not.toHaveBeenCalled();

    fireEvent.mouseDown(lane, { clientX: 100, button: 0 });
    fireEvent.mouseUp(lane, { clientX: 400, button: 0 });
    expect(onMoveClip).not.toHaveBeenCalled();
  });
});
