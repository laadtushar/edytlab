/**
 * ABCompareBar — unit tests.
 *
 * The Tauri bridge is fully mocked so the component can be exercised in
 * jsdom without a running Tauri webview.
 */

import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

// ---- mock tauri-bridge ----
const prepareCompareMock = vi.fn();
const acceptBMock = vi.fn();

vi.mock("../../lib/tauri-bridge", async () => {
  const actual = await vi.importActual<
    typeof import("../../lib/tauri-bridge")
  >("../../lib/tauri-bridge");
  return {
    ...actual,
    prepareCompare: (a: string, b: string) => prepareCompareMock(a, b),
    acceptB: (b: string) => acceptBMock(b),
  };
});

import { ABCompareBar } from "../ABCompareBar";

const A_NODE = "a".repeat(64);
const B_NODE = "b".repeat(64);
const A_PATH = "/tmp/compare_a.wav";
const B_PATH = "/tmp/compare_b.wav";

beforeEach(() => {
  prepareCompareMock.mockReset();
  acceptBMock.mockReset();
});

describe("ABCompareBar", () => {
  it("renders A and B buttons", async () => {
    prepareCompareMock.mockResolvedValue({ a_path: A_PATH, b_path: B_PATH });

    render(
      <ABCompareBar
        aNodeId={A_NODE}
        bNodeId={B_NODE}
        onAudioPathChange={vi.fn()}
        onAcceptB={vi.fn()}
        onClose={vi.fn()}
      />,
    );

    expect(screen.getByTestId("ab-side-a")).toBeInTheDocument();
    expect(screen.getByTestId("ab-side-b")).toBeInTheDocument();
  });

  it("shows Preparing… state on mount while prepareCompare is pending", () => {
    // Never resolve so we stay in the preparing state.
    prepareCompareMock.mockReturnValue(new Promise(() => undefined));

    render(
      <ABCompareBar
        aNodeId={A_NODE}
        bNodeId={B_NODE}
        onAudioPathChange={vi.fn()}
        onAcceptB={vi.fn()}
        onClose={vi.fn()}
      />,
    );

    // The centre label should read "Preparing…" and the A/B buttons
    // should be disabled while the render is in flight.
    expect(screen.getByTestId("ab-switch-label").textContent).toContain(
      "Preparing",
    );
    expect(screen.getByTestId("ab-side-a")).toBeDisabled();
    expect(screen.getByTestId("ab-side-b")).toBeDisabled();
  });

  it("calls onAudioPathChange with a_path after prepareCompare resolves (A is default)", async () => {
    prepareCompareMock.mockResolvedValue({ a_path: A_PATH, b_path: B_PATH });
    const onAudioPathChange = vi.fn();

    render(
      <ABCompareBar
        aNodeId={A_NODE}
        bNodeId={B_NODE}
        onAudioPathChange={onAudioPathChange}
        onAcceptB={vi.fn()}
        onClose={vi.fn()}
      />,
    );

    await waitFor(() => {
      expect(onAudioPathChange).toHaveBeenCalledWith(A_PATH);
    });
  });

  it("switches to B side and calls onAudioPathChange with b_path", async () => {
    prepareCompareMock.mockResolvedValue({ a_path: A_PATH, b_path: B_PATH });
    const onAudioPathChange = vi.fn();

    render(
      <ABCompareBar
        aNodeId={A_NODE}
        bNodeId={B_NODE}
        onAudioPathChange={onAudioPathChange}
        onAcceptB={vi.fn()}
        onClose={vi.fn()}
      />,
    );

    // Wait for preparing to finish.
    await waitFor(() => {
      expect(screen.getByTestId("ab-switch-label").textContent).not.toContain(
        "Preparing",
      );
    });

    fireEvent.click(screen.getByTestId("ab-side-b"));
    expect(onAudioPathChange).toHaveBeenLastCalledWith(B_PATH);
  });

  it("Accept B button calls acceptB then onAcceptB", async () => {
    prepareCompareMock.mockResolvedValue({ a_path: A_PATH, b_path: B_PATH });
    acceptBMock.mockResolvedValue(B_NODE);
    const onAcceptB = vi.fn();

    render(
      <ABCompareBar
        aNodeId={A_NODE}
        bNodeId={B_NODE}
        onAudioPathChange={vi.fn()}
        onAcceptB={onAcceptB}
        onClose={vi.fn()}
      />,
    );

    // Wait for preparing to finish.
    await waitFor(() =>
      expect(screen.getByTestId("ab-accept-b")).not.toBeDisabled(),
    );

    await act(async () => {
      fireEvent.click(screen.getByTestId("ab-accept-b"));
    });

    expect(acceptBMock).toHaveBeenCalledWith(B_NODE);
    expect(onAcceptB).toHaveBeenCalled();
  });

  it("Close button calls onClose", async () => {
    prepareCompareMock.mockResolvedValue({ a_path: A_PATH, b_path: B_PATH });
    const onClose = vi.fn();

    render(
      <ABCompareBar
        aNodeId={A_NODE}
        bNodeId={B_NODE}
        onAudioPathChange={vi.fn()}
        onAcceptB={vi.fn()}
        onClose={onClose}
      />,
    );

    fireEvent.click(screen.getByTestId("ab-close"));
    expect(onClose).toHaveBeenCalled();
  });
});
