/**
 * The handlers in `App` that send their command through `useSession`
 * (ADR 0001): a template, the track-head actions, Save As, a take that
 * could not be kept, and a punch-in that landed. Each is one `edit` or
 * `move`, and what the module does with it (adopt the head, read the
 * tracks again, let go of held controls) is `useSession.session.test.tsx`;
 * these pin that App hands the right command over, and says when it failed.
 *
 * The real `App` runs with only the IPC boundary replaced.
 */

import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { clearMocks } from "@tauri-apps/api/mocks";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("wavesurfer.js", async () => (await import("./wavesurferMock")).wavesurferModule());

import { type Args, appAnswer, bootIpc, node, quiet, settle, voiceTrack } from "./appHarness";
import App from "../App";

const A = node(1);
const headLabel = (id: string): string => `head ${id.slice(0, 7)}`;

class Backend {
  calls = 0;
  head: string | null = A;
  tracks = [voiceTrack()];
  /** Every command the app sent, in order. */
  readonly sent: { cmd: string; args: Args }[] = [];
  /** Answers a test sets for one command. */
  readonly answers = new Map<string, (args: Args) => unknown>();
  private next = 10;

  count(cmd: string): number {
    return this.sent.filter((s) => s.cmd === cmd).length;
  }

  /** The head the next edit makes. */
  advance(): string {
    this.head = node(this.next++);
    return this.head;
  }

  handle = (cmd: string, args: Args = {}): unknown => {
    this.calls++;
    this.sent.push({ cmd, args });
    const answer = this.answers.get(cmd);
    if (answer) return answer(args);
    switch (cmd) {
      case "list_tracks":
        return this.tracks;
      case "get_session_head":
        return this.head;
      case "get_head_trail":
        return { head: null, trail: [], redo: [] };
      case "save_head_trail":
        return null;
      case "get_graph":
        return { nodes: [], head: this.head };
      case "list_templates":
        return [{ name: "podcast", description: "Two voices" }];
      case "apply_template":
      case "rename_track":
      case "duplicate_track":
      case "remove_track":
        return this.advance();
      default:
        return appAnswer(cmd);
    }
  };
}

let backend: Backend;

async function start(): Promise<void> {
  render(<App />);
  await waitFor(() =>
    expect(screen.getByTestId("status-bar-head")).toHaveTextContent(headLabel(A)),
  );
  await quiet(backend);
}

const banner = () => screen.findByTestId("render-error");

beforeEach(() => {
  backend = new Backend();
  bootIpc(backend);
});

afterEach(() => {
  cleanup();
  clearMocks();
  vi.restoreAllMocks();
});

describe("a template", () => {
  it("is applied as an edit, whose head the window adopts", async () => {
    backend.head = null;
    backend.tracks = [];
    render(<App />);
    fireEvent.click(await screen.findByTestId("empty-state-templates-button"));
    fireEvent.click(await screen.findByRole("button", { name: /podcast/ }));

    await waitFor(() => expect(backend.count("apply_template")).toBe(1));
    expect(backend.sent.find((s) => s.cmd === "apply_template")?.args).toEqual({ name: "podcast" });
    await waitFor(() =>
      expect(screen.getByTestId("status-bar-head")).toHaveTextContent(headLabel(backend.head!)),
    );
  });

  it("that the session refuses says why", async () => {
    backend.head = null;
    backend.tracks = [];
    backend.answers.set("apply_template", () => {
      throw "no such template";
    });
    render(<App />);
    fireEvent.click(await screen.findByTestId("empty-state-templates-button"));
    fireEvent.click(await screen.findByRole("button", { name: /podcast/ }));

    expect(await banner()).toHaveTextContent("no such template");
  });
});

describe("the track-head actions (#161)", () => {
  it.each([
    ["duplicate", "track-duplicate-0", "duplicate_track", { track: 0 }],
    ["remove", "track-remove-0", "remove_track", { track: 0 }],
  ])("%s is one edit, and its head is adopted", async (_, item, cmd, args) => {
    await start();
    fireEvent.click(screen.getAllByTestId("track-menu-btn-0")[0]);
    fireEvent.click(screen.getByTestId(item));

    await waitFor(() => expect(backend.count(cmd)).toBe(1));
    expect(backend.sent.find((s) => s.cmd === cmd)?.args).toEqual(args);
    await waitFor(() =>
      expect(screen.getByTestId("status-bar-head")).toHaveTextContent(headLabel(backend.head!)),
    );
  });

  it("rename is one edit with the name typed", async () => {
    await start();
    fireEvent.click(screen.getAllByTestId("track-menu-btn-0")[0]);
    fireEvent.click(screen.getByTestId("track-rename-0"));
    const input = screen.getByTestId("track-rename-input-0");
    fireEvent.change(input, { target: { value: "Host" } });
    fireEvent.keyDown(input, { key: "Enter" });

    await waitFor(() => expect(backend.count("rename_track")).toBe(1));
    expect(backend.sent.find((s) => s.cmd === "rename_track")?.args).toEqual({
      track: 0,
      name: "Host",
    });
  });

  it("a refused one says why", async () => {
    backend.answers.set("remove_track", () => {
      throw "track 0 does not exist";
    });
    await start();
    fireEvent.click(screen.getAllByTestId("track-menu-btn-0")[0]);
    fireEvent.click(screen.getByTestId("track-remove-0"));

    expect(await banner()).toHaveTextContent("track 0 does not exist");
  });
});

describe("Save As", () => {
  it("copies the project to the folder picked, as a move in the line", async () => {
    backend.answers.set("plugin:dialog|open", () => "/tmp/copy");
    backend.answers.set("save_project_as", () => ({
      files: 3,
      bytes: 2_000_000,
      skipped_cache_files: 1,
    }));
    const info = vi.spyOn(console, "info").mockImplementation(() => undefined);
    await start();

    fireEvent.click(screen.getByTestId("save-as-button"));

    await waitFor(() => expect(backend.count("save_project_as")).toBe(1));
    expect(backend.sent.find((s) => s.cmd === "save_project_as")?.args).toEqual({
      dest: "/tmp/copy",
    });
    await waitFor(() => expect(info).toHaveBeenCalledWith(expect.stringContaining("/tmp/copy")));
    // The recents list is read again for the copy.
    expect(backend.count("list_recent_projects")).toBeGreaterThan(1);
  });
});

describe("a take that could not be kept", () => {
  it("says so when the recording could not be saved", async () => {
    backend.answers.set("start_recording", () => "recording");
    backend.answers.set("stop_recording", () => {
      throw "disk full";
    });
    await start();

    fireEvent.click(screen.getByTestId("record-btn"));
    await waitFor(() => expect(screen.getByTestId("record-btn")).toHaveTextContent("Stop"));
    fireEvent.click(screen.getByTestId("record-btn"));

    expect(await banner()).toHaveTextContent("could not be saved");
    expect(backend.count("batch_load")).toBe(0);
  });

  it("says so when a scheduled take failed", async () => {
    backend.answers.set("timer_record", () => {
      throw "no microphone";
    });
    await start();

    fireEvent.click(screen.getByTestId("timer-record-btn"));
    fireEvent.change(screen.getByTestId("timer-duration-sec"), { target: { value: "5" } });
    fireEvent.click(screen.getByTestId("timer-record-arm"));

    expect(await banner()).toHaveTextContent("no microphone");
  });
});

describe("a punch-in that landed", () => {
  it("reads the head it left in the line, and says how it was placed", async () => {
    const punched = node(50);
    backend.answers.set("get_view_state", () => ({ selection: [1, 2] }));
    backend.answers.set("get_recording_latency", () => ({
      devices: { host: "ALSA", output: "out", input: "in", out_sr: 48000, in_sr: 48000 },
      problem: null,
      latency: { latency_ms: 12, measured_at: Math.floor(Date.now() / 1000), source: "manual" },
    }));
    backend.answers.set("punch_record", () => {
      backend.head = punched;
      return {
        cancelled: false,
        head: punched,
        latency_ms: 12,
        latency_source: "manual",
      };
    });
    await start();

    fireEvent.click(screen.getByTestId("punch-btn"));
    const panel = await screen.findByTestId("punch-panel");
    const go = within(panel).getByTestId("punch-start");
    await waitFor(() => expect(go).toBeEnabled());
    await act(async () => {
      fireEvent.click(go);
    });
    await settle();

    // The head is the one the store reports after the punch, read in the line.
    await waitFor(() =>
      expect(screen.getByTestId("status-bar-head")).toHaveTextContent(headLabel(punched)),
    );
    expect(backend.count("punch_record")).toBe(1);
    // Applied in the project it was asked in: the notice says how.
    expect(await screen.findByTestId("punch-notice")).toBeInTheDocument();
    expect(screen.queryByTestId("render-error")).not.toBeInTheDocument();
  });
});
