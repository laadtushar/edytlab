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

const cbs = {
  textDelta: [] as ((text: string) => void)[],
  toolCall: [] as ((name: string, id: string) => void)[],
  toolCallEnd: [] as ((id: string, ok: boolean) => void)[],
  nodeCreated: [] as ((nodeId: string) => void)[],
  done: [] as (() => void)[],
  plan: [] as ((steps: Record<string, unknown>[]) => void)[],
};

const sendMessageMock = vi.fn();

vi.mock("../lib/tauri-bridge", () => ({
  sendMessage: (text: string) => sendMessageMock(text),
  approvePlan: vi.fn(() => Promise.resolve()),
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
  onToolCallEnd: vi.fn((cb: (id: string, ok: boolean) => void) => {
    cbs.toolCallEnd.push(cb);
    return Promise.resolve(() => undefined);
  }),
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
