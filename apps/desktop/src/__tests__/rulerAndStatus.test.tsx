/**
 * Two more found by loading a 3-second file into the built app.
 *
 * Neither is exotic. Both are what the app shows you the moment audio
 * arrives, and both were wrong.
 */

import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { Ruler } from "../components/Ruler";
import { StatusBar } from "../App";

function tickLabels(): string[] {
  return Array.from(
    screen.getByTestId("ruler").querySelectorAll("span"),
  )
    .map((el) => el.textContent ?? "")
    .filter((t) => t.length > 0);
}

describe("the time ruler", () => {
  /**
   * Six ticks are drawn across whatever duration the ruler is given,
   * so the gap between them shrinks with the file. `fmtTimecode`
   * floored to whole seconds, so a 3-second file ticked every 0.5s
   * printed each label twice:
   *
   *   0:00  0:00  0:01  0:01  0:02  0:02
   *
   * Four of those six labels sat somewhere other than where they said.
   */
  it("gives every tick a distinct label on a short file", () => {
    render(<Ruler duration={3} />);
    const labels = tickLabels();
    expect(labels.length).toBeGreaterThan(1);
    expect(
      new Set(labels).size,
      `duplicate tick labels: ${labels.join(" ")}`,
    ).toBe(labels.length);
  });

  it("uses sub-second precision only when the ticks need it", () => {
    render(<Ruler duration={3} />);
    expect(tickLabels()[1]).toMatch(/^\d+:\d\d\.\d$/);
  });

  it("stays on whole seconds when the ticks are a second or more apart", () => {
    render(<Ruler duration={600} />);
    const labels = tickLabels();
    expect(labels[1]).toMatch(/^\d+:\d\d$/);
    expect(new Set(labels).size).toBe(labels.length);
  });

  it("does not print a sixtieth second", () => {
    // `toFixed` rounds 59.97 to "60.0", which would read 0:60.0.
    render(<Ruler duration={59.97 * 6} />);
    for (const label of tickLabels()) {
      const seconds = label.split(":")[1];
      expect(Number.parseFloat(seconds)).toBeLessThan(60);
    }
  });

  it("survives a zero duration", () => {
    render(<Ruler duration={0} />);
    expect(screen.getByTestId("ruler")).toBeInTheDocument();
  });
});

describe("the status bar", () => {
  /**
   * State came from `audioPath` alone — from a path having been
   * *chosen*. A file that 404'd left "ready" and the filename sitting
   * directly under the timeline's own error box: two claims about the
   * same file, and the one the user reads first was wrong.
   */
  it("says the load failed rather than ready", () => {
    render(
      <StatusBar
        audioPath="/tmp/gone.wav"
        head={null}
        rendering={false}
        selection={null}
        loadError="404 (Not Found)"
      />,
    );
    const text = screen.getByTestId("status-bar").textContent ?? "";
    expect(text).toMatch(/load failed/i);
    expect(text).not.toMatch(/\bready\b/i);
  });

  it("still says ready when the audio loaded", () => {
    render(
      <StatusBar
        audioPath="/tmp/ok.wav"
        head={null}
        rendering={false}
        selection={null}
        loadError={null}
      />,
    );
    expect(screen.getByTestId("status-bar").textContent).toMatch(/ready/i);
  });

  it("is idle with no file, error or not", () => {
    render(
      <StatusBar
        audioPath={null}
        head={null}
        rendering={false}
        selection={null}
        loadError="stale error"
      />,
    );
    const text = screen.getByTestId("status-bar").textContent ?? "";
    expect(text).toMatch(/idle/i);
    expect(text).not.toMatch(/load failed/i);
  });

  it("lets a render in progress win over a stale error", () => {
    render(
      <StatusBar
        audioPath="/tmp/a.wav"
        head={null}
        rendering
        selection={null}
        loadError="404"
      />,
    );
    expect(screen.getByTestId("status-bar").textContent).toMatch(/rendering/i);
  });

  /**
   * #416. After a destructive edit the track reads its audio from
   * `.audiograph/derived/<hash>.wav`, and the bar named the session after
   * that file: `703E2A7B…A370E.WAV`. It names the track instead.
   */
  describe("after a destructive edit", () => {
    const HASH = "703e2a7b07919f079abdfbd8c3556c274c8d519c40ef72fe75586ba5bada370e";

    function fileLabel(audioPath: string, trackName?: string): HTMLElement {
      render(
        <StatusBar
          audioPath={audioPath}
          trackName={trackName}
          head={null}
          rendering={false}
          selection={null}
        />,
      );
      return screen.getByTestId("status-bar-file");
    }

    it("names the track rather than the derived file", () => {
      const label = fileLabel(
        `/home/me/Music/song/.audiograph/derived/${HASH}.wav`,
        "music-8s-stereo",
      );
      expect(label).toHaveTextContent("music-8s-stereo");
      expect(label.textContent).not.toMatch(/[0-9a-f]{12}/i);
      // Nor in the tooltip, which used to hold the whole path.
      expect(label.getAttribute("title") ?? "").not.toMatch(/[0-9a-f]{12}/i);
    });

    it("does the same on Windows", () => {
      const label = fileLabel(
        `C:\\Users\\tusha\\Music\\song\\.audiograph\\derived\\${HASH}.wav`,
        "Lead guitar",
      );
      expect(label).toHaveTextContent("Lead guitar");
      expect(label.textContent).not.toMatch(/[0-9a-f]{12}/i);
    });

    /** A track cut into clips is drawn from a flattened file there too. */
    it("names the track for a flattened lane file", () => {
      const label = fileLabel(
        `/home/me/Music/song/.audiograph/derived/track-${HASH}.wav`,
        "Lead guitar",
      );
      expect(label).toHaveTextContent("Lead guitar");
      expect(label.textContent).not.toMatch(/[0-9a-f]{12}/i);
    });

    it("never falls back to the hash when the track has no name", () => {
      const label = fileLabel(`/home/me/Music/song/.audiograph/derived/${HASH}.wav`);
      expect(label.textContent).not.toMatch(/[0-9a-f]{12}/i);
      expect(label.textContent).not.toBe("");
    });

    /** What it has always shown for a file the user loaded. */
    it("still names a file the user loaded by its file name", () => {
      const label = fileLabel("/home/me/Music/music.wav", "music");
      expect(label).toHaveTextContent("music.wav");
      expect(label).toHaveAttribute("title", "/home/me/Music/music.wav");
    });
  });

  /**
   * An undo onto swept history rebuilds its audio first (#373). The
   * state word says so rather than `ready` while the key looks dead.
   */
  it("says history is being restored while a slow undo runs", () => {
    render(
      <StatusBar
        audioPath="/tmp/a.wav"
        head={null}
        rendering={false}
        selection={null}
        loadError={null}
        restoringHistory
      />,
    );
    const state = screen.getByRole("status");
    expect(state).toHaveTextContent(/restoring history/i);
    expect(state).not.toHaveTextContent(/ready/i);
  });
});
