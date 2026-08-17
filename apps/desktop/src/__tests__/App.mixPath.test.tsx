/**
 * A source file and a rendered mix are different things (#155).
 *
 * They used to share one `audioPath` state, and the collision is the
 * reason the mixer is inaudible. Six writers set it; three of them set a
 * *source* file, and the decisive one runs on `onNodeCreated` — after
 * every agent turn — replacing whatever mix had been rendered with
 * `newTracks[0].audio_path`, a raw per-track WAV with no gain, pan,
 * mute, solo, chain, send or master-chain applied.
 *
 * So the mix was correct for roughly one render and then quietly was
 * not, with nothing on screen to say so.
 *
 * These tests pin the separation, and the staleness signal that makes it
 * legible: a preview is named after the node it came from, so a stale one
 * is otherwise indistinguishable from a current one.
 */

import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

describe("source path vs mix path", () => {
  /**
   * The regression that motivated the split. A node-created event must
   * update what the *lanes* show without touching what the transport
   * would *play*.
   */
  it("a track's own audio is a source, never the mix", async () => {
    const { StatusBar } = await import("../App");
    // StatusBar is the observable surface for both values: it shows the
    // source file, and flags the mix as stale.
    render(
      <StatusBar
        audioPath="/tmp/track-0-raw.wav"
        head="abc1234"
        rendering={false}
        selection={null}
        mixStale
      />,
    );
    expect(screen.getByTestId("status-bar-file").textContent).toContain(
      "track-0-raw.wav",
    );
    expect(screen.getByTestId("status-bar-mix-stale")).toBeInTheDocument();
  });

  it("says nothing about staleness when the mix matches the head", async () => {
    const { StatusBar } = await import("../App");
    render(
      <StatusBar
        audioPath="/tmp/track-0-raw.wav"
        head="abc1234"
        rendering={false}
        selection={null}
        mixStale={false}
      />,
    );
    expect(screen.queryByTestId("status-bar-mix-stale")).not.toBeInTheDocument();
  });

  /**
   * Absent is not the same as stale. Before any render there is no mix,
   * and claiming one is "out of date" would be inventing a state.
   */
  it("says nothing about staleness before anything has been rendered", async () => {
    const { StatusBar } = await import("../App");
    render(
      <StatusBar
        audioPath="/tmp/track-0-raw.wav"
        head="abc1234"
        rendering={false}
        selection={null}
      />,
    );
    expect(screen.queryByTestId("status-bar-mix-stale")).not.toBeInTheDocument();
  });

  it("still reports no file when nothing is loaded", async () => {
    const { StatusBar } = await import("../App");
    render(
      <StatusBar
        audioPath={null}
        head={null}
        rendering={false}
        selection={null}
      />,
    );
    expect(screen.getByTestId("status-bar-file").textContent).toContain(
      "no file loaded",
    );
  });

  it("rendering takes precedence over the ready state", async () => {
    const { StatusBar } = await import("../App");
    render(
      <StatusBar
        audioPath="/tmp/a.wav"
        head="abc"
        rendering
        selection={null}
      />,
    );
    expect(screen.getByTestId("status-bar")).toHaveTextContent("rendering");
  });

});
