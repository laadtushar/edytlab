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
});
