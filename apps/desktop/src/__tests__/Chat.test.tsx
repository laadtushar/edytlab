/**
 * Chat — basic rendering, input handling, and bridge integration.
 *
 * The bridge is mocked so we can assert that submitting the form calls
 * `sendMessage` with the typed text, and so the agent event listeners
 * can be driven directly from the test.
 */

import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { ToolView } from "../lib/tauri-bridge";

const cbs = {
  textDelta: [] as ((text: string) => void)[],
  toolCall: [] as ((name: string, id: string) => void)[],
  toolCallEnd: [] as ((id: string, ok: boolean, view?: ToolView) => void)[],
  nodeCreated: [] as ((nodeId: string) => void)[],
  done: [] as (() => void)[],
  plan: [] as ((steps: Record<string, unknown>[]) => void)[],
  planUnavailable: [] as ((reason: string) => void)[],
};

const sendMessageMock = vi.fn();

vi.mock("../lib/tauri-bridge", () => ({
  sendMessage: (text: string) => sendMessageMock(text),
  approvePlan: vi.fn(() => Promise.resolve()),
  rejectPlan: vi.fn(() => Promise.resolve()),
  getPlanFirst: vi.fn(() => Promise.resolve(false)),
  setPlanFirst: vi.fn(() => Promise.resolve()),
  listCapabilities: vi.fn(() =>
    Promise.resolve({
      tools: [
        {
          name: "render_preview",
          description: "Preview the rendered audio.",
          category: "session",
        },
      ],
      skills: [],
      agents: [],
      mcp_servers: [],
    }),
  ),
  onTextDelta: vi.fn((cb: (t: string) => void) => {
    cbs.textDelta.push(cb);
    return Promise.resolve(() => undefined);
  }),
  onToolCall: vi.fn((cb: (n: string, i: string) => void) => {
    cbs.toolCall.push(cb);
    return Promise.resolve(() => undefined);
  }),
  onToolCallEnd: vi.fn(
    (cb: (id: string, ok: boolean, view?: ToolView) => void) => {
      cbs.toolCallEnd.push(cb);
      return Promise.resolve(() => undefined);
    },
  ),
  onNodeCreated: vi.fn((cb: (n: string) => void) => {
    cbs.nodeCreated.push(cb);
    return Promise.resolve(() => undefined);
  }),
  onAgentDone: vi.fn((cb: () => void) => {
    cbs.done.push(cb);
    return Promise.resolve(() => undefined);
  }),
  onPlan: vi.fn((cb: (steps: Record<string, unknown>[]) => void) => {
    cbs.plan.push(cb);
    return Promise.resolve(() => undefined);
  }),
  onPlanUnavailable: vi.fn((cb: (reason: string) => void) => {
    cbs.planUnavailable.push(cb);
    return Promise.resolve(() => undefined);
  }),
}));

import { Chat } from "../components/Chat";

const flush = () => new Promise<void>((r) => setTimeout(r, 0));

describe("Chat", () => {
  beforeEach(() => {
    sendMessageMock.mockReset();
    sendMessageMock.mockResolvedValue(undefined);
    cbs.textDelta = [];
    cbs.toolCall = [];
    cbs.toolCallEnd = [];
    cbs.nodeCreated = [];
    cbs.done = [];
    cbs.plan = [];
    cbs.planUnavailable = [];
  });

  it("renders the input, send button, and Render Preview button", () => {
    render(<Chat />);
    expect(screen.getByLabelText("Message")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: /send/i }),
    ).toBeInTheDocument();
    expect(screen.getByTestId("render-preview-button")).toBeInTheDocument();
  });

  it("sends a typed message via the bridge and shows it in the log", async () => {
    const user = userEvent.setup();
    render(<Chat />);
    await act(async () => {
      await flush();
    });

    const input = screen.getByLabelText("Message");
    await user.type(input, "normalize to -1 dBFS");
    await user.click(screen.getByRole("button", { name: /send/i }));

    expect(sendMessageMock).toHaveBeenCalledWith("normalize to -1 dBFS");
    const bubbles = screen.getAllByTestId("message-bubble");
    expect(bubbles.some((b) => b.dataset.role === "user")).toBe(true);
    expect(bubbles[bubbles.length - 1].textContent).toContain(
      "normalize to -1 dBFS",
    );
  });

  it("renders streamed assistant deltas and a running tool badge", async () => {
    render(<Chat />);
    await act(async () => {
      await flush();
    });

    await act(async () => {
      cbs.textDelta[0]("loaded foo.wav");
      cbs.toolCall[0]("normalize", "tool-1");
    });

    expect(screen.getByText(/loaded foo\.wav/)).toBeInTheDocument();
    const badge = screen.getByTestId("tool-badge");
    expect(badge).toHaveAttribute("data-status", "running");
  });

  /**
   * The unavailable card is the whole user-facing half of #233, and it
   * is reached by *matching the backend's error text*. That coupling is
   * invisible to the compiler: change the wording on the Rust side and
   * the card silently stops appearing, leaving the user with a raw
   * error and no explanation. Raised in review on #317.
   *
   * Each string below is one of the three the real backend produces.
   * They are asserted separately so a failure names which one drifted
   * rather than just reporting that the card is gone.
   */
  describe("the transcription-unavailable card", () => {
    const BACKEND_ERRORS = {
      "the stub error from ml-whisper":
        "Could not complete request: speech-to-text is not implemented in this build",
      "the missing-model error": "Whisper model not found at /models/x.onnx",
      "a message naming the env var":
        "set WHISPER_MODEL_PATH to an ONNX Whisper export",
    };

    for (const [label, text] of Object.entries(BACKEND_ERRORS)) {
      it(`appears for ${label}`, async () => {
        render(<Chat />);
        await act(async () => {
          await flush();
        });
        await act(async () => {
          cbs.textDelta[0](text);
          // The in-flight bubble renders outside the entry list, so
          // the card appears once the turn commits rather than
          // flickering mid-stream. `done` is what commits it.
          cbs.done.forEach((cb) => cb());
        });

        const card = screen.getByTestId("whisper-unavailable-card");
        expect(card).toBeInTheDocument();
        expect(card.textContent).toContain("isn't available in this build");
        // The card exists to say no setup helps. If it ever tells the
        // user to install something, it has become the dead end it
        // replaced.
        expect(card.textContent).not.toMatch(/fetch-models|install/i);
      });
    }

    it("stays away for an ordinary assistant message", async () => {
      render(<Chat />);
      await act(async () => {
        await flush();
      });
      await act(async () => {
        cbs.textDelta[0]("normalized track 1 to -1 dBFS");
        cbs.done.forEach((cb) => cb());
      });

      expect(
        screen.queryByTestId("whisper-unavailable-card"),
      ).not.toBeInTheDocument();
    });
  });

  /**
   * `plot_spectrum` computed a curve, the backend threw it away, and the
   * chart component sat in the tree with no call site — the whole
   * feature was unreachable from the app. This drives the same event the
   * real bridge does.
   */
  it("draws the spectrum chart when a tool call returns one", async () => {
    render(<Chat />);
    await act(async () => {
      await flush();
    });

    await act(async () => {
      cbs.toolCall[0]("plot_spectrum", "tool-1");
      cbs.toolCallEnd[0]("tool-1", true, {
        type: "spectrum",
        points: [
          { hz: 0, db: -120 },
          { hz: 440, db: -6 },
          { hz: 8000, db: -80 },
        ],
        summary: "Spectrum for track 0 (0.00s..0.50s), 3 bins",
      });
    });

    expect(
      screen.getByTestId("spectrum-chart"),
      "the tool returned a spectrum and nothing was drawn",
    ).toBeInTheDocument();
    expect(screen.getByTestId("spectrum-caption")).toHaveTextContent(
      /Spectrum for track 0/,
    );
    // The canvas is opaque to assistive tech unless we say what's in it.
    expect(screen.getByRole("img")).toHaveAttribute(
      "aria-label",
      expect.stringContaining("440"),
    );
  });

  it("leaves the badge alone for tools that return no chart", async () => {
    render(<Chat />);
    await act(async () => {
      await flush();
    });

    await act(async () => {
      cbs.toolCall[0]("normalize", "tool-1");
      cbs.toolCallEnd[0]("tool-1", true);
    });

    expect(screen.getByTestId("tool-badge")).toHaveAttribute(
      "data-status",
      "ok",
    );
    expect(screen.queryByTestId("spectrum-chart")).not.toBeInTheDocument();
  });

  /**
   * Two tool calls in flight at once must not cross their results — the
   * chart hangs off the id, same as the badge status does.
   */
  it("attaches the chart to the call that produced it", async () => {
    render(<Chat />);
    await act(async () => {
      await flush();
    });

    await act(async () => {
      cbs.toolCall[0]("normalize", "tool-1");
      cbs.toolCall[0]("plot_spectrum", "tool-2");
      cbs.toolCallEnd[0]("tool-2", true, {
        type: "spectrum",
        points: [{ hz: 440, db: -6 }],
      });
    });

    const charts = screen.getAllByTestId("spectrum-chart");
    expect(charts).toHaveLength(1);
    const badges = screen.getAllByTestId("tool-badge");
    expect(badges[0]).toHaveAttribute("data-status", "running");
    expect(badges[1]).toHaveAttribute("data-status", "ok");
    // The chart belongs to the second badge's row, not the first.
    expect(badges[1].closest("div")?.parentElement).toContainElement(charts[0]);
  });

  it("renders a node-created divider when the agent emits one", async () => {
    render(<Chat />);
    await act(async () => {
      await flush();
    });

    await act(async () => {
      cbs.toolCall[0]("normalize", "tool-1");
      cbs.nodeCreated[0]("a".repeat(64));
    });

    expect(screen.getByTestId("node-divider")).toBeInTheDocument();
  });

  it("surfaces a friendly error if sendMessage rejects", async () => {
    sendMessageMock.mockRejectedValueOnce("source is silent");
    const user = userEvent.setup();
    render(<Chat />);
    await act(async () => {
      await flush();
    });

    const input = screen.getByLabelText("Message");
    await user.type(input, "normalize");
    await user.click(screen.getByRole("button", { name: /send/i }));
    await act(async () => {
      await flush();
    });

    const err = screen.getByTestId("chat-error");
    expect(err.textContent).toContain("source is silent");
  });

  it("invokes onRequestRenderPreview when the button is clicked", async () => {
    const onClick = vi.fn();
    const user = userEvent.setup();
    render(<Chat onRequestRenderPreview={onClick} />);
    await user.click(screen.getByTestId("render-preview-button"));
    expect(onClick).toHaveBeenCalledTimes(1);
  });

  it("shows the thinking indicator while awaiting the first delta", async () => {
    const user = userEvent.setup();
    render(<Chat />);
    await act(async () => {
      await flush();
    });

    const input = screen.getByLabelText("Message");
    await user.type(input, "normalize to -1");
    await user.click(screen.getByRole("button", { name: /send/i }));
    await act(async () => {
      await flush();
    });
    expect(screen.getByTestId("thinking-indicator")).toBeInTheDocument();

    await act(async () => {
      cbs.textDelta[0]("ok");
    });
    expect(screen.queryByTestId("thinking-indicator")).not.toBeInTheDocument();
  });

  it("renders action chips on the last assistant message and resubmits on click", async () => {
    const user = userEvent.setup();
    render(<Chat />);
    await act(async () => {
      await flush();
    });

    // Drive an assistant turn that mentions `render_preview`.
    await act(async () => {
      cbs.textDelta[0](
        "Loaded track 0. You can `render_preview` or normalize.",
      );
      cbs.done[0]();
    });

    const chips = screen.getAllByTestId("message-chip");
    expect(chips.length).toBeGreaterThan(0);
    const previewChip = chips.find((c) =>
      c.getAttribute("data-chip-id")?.startsWith("render_preview"),
    );
    expect(previewChip).toBeDefined();

    sendMessageMock.mockReset();
    sendMessageMock.mockResolvedValue(undefined);
    await user.click(previewChip!);
    expect(sendMessageMock).toHaveBeenCalledTimes(1);
    expect(sendMessageMock.mock.calls[0][0]).toMatch(/preview/i);
  });

  it("opens the capabilities menu when the + toggle is clicked", async () => {
    const user = userEvent.setup();
    render(<Chat />);
    await act(async () => {
      await flush();
    });

    expect(screen.queryByTestId("capabilities-menu")).not.toBeInTheDocument();
    await user.click(screen.getByTestId("capabilities-toggle"));
    expect(screen.getByTestId("capabilities-menu")).toBeInTheDocument();
  });
});
