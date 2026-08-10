/**
 * McpServersEditor — list/CRUD smoke plus regression coverage for the
 * multi-line fields.
 *
 * `args`, `env`, and `headers` are edited as text but stored parsed
 * (`string[]` / `Record<string, string>`). Rendering the textarea from
 * the parsed value makes in-progress edits unrepresentable — a key
 * typed before its `=`, or a newline before the next arg — so those
 * fields keep raw text alongside. The typing tests below are what pin
 * that down; they fail against a parse-on-render implementation.
 */

import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

const listMcpServersMock = vi.fn();
const readMcpServerMock = vi.fn();
const upsertMcpServerMock = vi.fn();
const deleteMcpServerMock = vi.fn();
const restartMcpServerMock = vi.fn();

vi.mock("../../lib/tauri-bridge", () => ({
  listMcpServers: () => listMcpServersMock(),
  readMcpServer: (id: string) => readMcpServerMock(id),
  upsertMcpServer: (id: string, entry: unknown) =>
    upsertMcpServerMock(id, entry),
  deleteMcpServer: (id: string) => deleteMcpServerMock(id),
  restartMcpServer: (id: string) => restartMcpServerMock(id),
}));

import { McpServersEditor } from "../McpServersEditor";

const LIST_ENTRY = {
  id: "github",
  transport: "stdio" as const,
  enabled: true,
  status: "running" as const,
  tools_count: 3,
  last_error: null,
};

const SERVER = {
  id: "github",
  transport: "stdio" as const,
  command: "npx",
  args: ["-y", "@modelcontextprotocol/server-github"],
  env: { GITHUB_TOKEN: "<keychain:gh>" },
  url: "",
  headers: {},
  enabled: true,
};

describe("McpServersEditor", () => {
  beforeEach(() => {
    listMcpServersMock.mockReset().mockResolvedValue([LIST_ENTRY]);
    readMcpServerMock.mockReset().mockResolvedValue(SERVER);
    upsertMcpServerMock.mockReset().mockResolvedValue(undefined);
    deleteMcpServerMock.mockReset().mockResolvedValue(undefined);
    restartMcpServerMock.mockReset().mockResolvedValue(undefined);
    vi.spyOn(window, "confirm").mockReturnValue(true);
  });

  it("lists servers with status and tool count", async () => {
    render(<McpServersEditor />);
    await waitFor(() => expect(listMcpServersMock).toHaveBeenCalled());

    expect(await screen.findByTestId("mcp-row-github")).toBeInTheDocument();
    expect(screen.getByTestId("mcp-status-badge-running")).toBeInTheDocument();
  });

  it("opens a server and populates the form from disk", async () => {
    const user = userEvent.setup();
    render(<McpServersEditor />);
    await waitFor(() => expect(listMcpServersMock).toHaveBeenCalled());

    await user.click(await screen.findByTestId("mcp-row-github"));
    await waitFor(() => expect(readMcpServerMock).toHaveBeenCalledWith("github"));

    expect((screen.getByTestId("mcp-command") as HTMLInputElement).value).toBe(
      "npx",
    );
    expect((screen.getByTestId("mcp-args") as HTMLTextAreaElement).value).toBe(
      "-y\n@modelcontextprotocol/server-github",
    );
    expect((screen.getByTestId("mcp-env") as HTMLTextAreaElement).value).toBe(
      "GITHUB_TOKEN=<keychain:gh>",
    );
  });

  /**
   * Regression: parsing on every keystroke dropped empty lines, so
   * pressing Enter to start a second argument was a no-op — the two
   * args collapsed into one and the user could never split them.
   */
  it("allows typing a multi-line args list", async () => {
    const user = userEvent.setup();
    render(<McpServersEditor />);
    await waitFor(() => expect(listMcpServersMock).toHaveBeenCalled());

    await user.click(screen.getByTestId("mcp-new"));
    await user.type(screen.getByTestId("mcp-id"), "local");
    await user.type(screen.getByTestId("mcp-command"), "node");

    const args = screen.getByTestId("mcp-args") as HTMLTextAreaElement;
    await user.type(args, "--flag{enter}server.js");

    expect(args.value).toBe("--flag\nserver.js");

    await user.click(screen.getByTestId("mcp-save"));
    await waitFor(() => expect(upsertMcpServerMock).toHaveBeenCalled());
    const [, entry] = upsertMcpServerMock.mock.calls[0];
    expect((entry as { args: string[] }).args).toEqual(["--flag", "server.js"]);
  });

  /**
   * Regression: an env line is only parseable once its `=` exists, so
   * parse-on-render discarded every keystroke of the key. The field was
   * impossible to fill in at all.
   */
  it("allows typing an env var one character at a time", async () => {
    const user = userEvent.setup();
    render(<McpServersEditor />);
    await waitFor(() => expect(listMcpServersMock).toHaveBeenCalled());

    await user.click(screen.getByTestId("mcp-new"));
    await user.type(screen.getByTestId("mcp-id"), "local");
    await user.type(screen.getByTestId("mcp-command"), "node");

    const env = screen.getByTestId("mcp-env") as HTMLTextAreaElement;
    await user.type(env, "TOKEN=abc123");

    expect(env.value).toBe("TOKEN=abc123");

    await user.click(screen.getByTestId("mcp-save"));
    await waitFor(() => expect(upsertMcpServerMock).toHaveBeenCalled());
    const [, entry] = upsertMcpServerMock.mock.calls[0];
    expect((entry as { env: Record<string, string> }).env).toEqual({
      TOKEN: "abc123",
    });
  });

  it("allows typing SSE headers", async () => {
    const user = userEvent.setup();
    render(<McpServersEditor />);
    await waitFor(() => expect(listMcpServersMock).toHaveBeenCalled());

    await user.click(screen.getByTestId("mcp-new"));
    await user.type(screen.getByTestId("mcp-id"), "remote");
    await user.selectOptions(screen.getByTestId("mcp-transport"), "sse");
    await user.type(screen.getByTestId("mcp-url"), "https://example.com/sse");

    const headers = screen.getByTestId("mcp-headers") as HTMLTextAreaElement;
    await user.type(headers, "Authorization: Bearer xyz");

    expect(headers.value).toBe("Authorization: Bearer xyz");

    await user.click(screen.getByTestId("mcp-save"));
    await waitFor(() => expect(upsertMcpServerMock).toHaveBeenCalled());
    const [, entry] = upsertMcpServerMock.mock.calls[0];
    expect((entry as { headers: Record<string, string> }).headers).toEqual({
      Authorization: "Bearer xyz",
    });
  });

  /**
   * Switching servers must re-derive the raw text from the newly loaded
   * entry rather than leaving the previous server's text on screen.
   */
  it("refreshes the form when a different server is opened", async () => {
    const user = userEvent.setup();
    listMcpServersMock.mockResolvedValue([
      LIST_ENTRY,
      { ...LIST_ENTRY, id: "other", tools_count: 1 },
    ]);
    readMcpServerMock.mockImplementation((id: string) =>
      Promise.resolve(
        id === "github"
          ? SERVER
          : { ...SERVER, id: "other", command: "python", args: ["-m", "srv"] },
      ),
    );

    render(<McpServersEditor />);
    await waitFor(() => expect(listMcpServersMock).toHaveBeenCalled());

    await user.click(await screen.findByTestId("mcp-row-github"));
    await waitFor(() =>
      expect(
        (screen.getByTestId("mcp-args") as HTMLTextAreaElement).value,
      ).toContain("@modelcontextprotocol"),
    );

    await user.click(screen.getByTestId("mcp-row-other"));
    await waitFor(() =>
      expect((screen.getByTestId("mcp-args") as HTMLTextAreaElement).value).toBe(
        "-m\nsrv",
      ),
    );
  });

  it("restarts a server from the list row", async () => {
    const user = userEvent.setup();
    render(<McpServersEditor />);
    await waitFor(() => expect(listMcpServersMock).toHaveBeenCalled());

    await user.click(await screen.findByTestId("mcp-restart-github"));
    await waitFor(() =>
      expect(restartMcpServerMock).toHaveBeenCalledWith("github"),
    );
  });

  it("deletes a server after confirmation", async () => {
    const user = userEvent.setup();
    render(<McpServersEditor />);
    await waitFor(() => expect(listMcpServersMock).toHaveBeenCalled());

    await user.click(await screen.findByTestId("mcp-row-github"));
    await waitFor(() => expect(readMcpServerMock).toHaveBeenCalled());

    await user.click(screen.getByTestId("mcp-delete"));
    await waitFor(() =>
      expect(deleteMcpServerMock).toHaveBeenCalledWith("github"),
    );
  });
});
