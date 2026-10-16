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
const endCompareMock = vi.fn();

vi.mock("../../lib/tauri-bridge", async () => {
  const actual = await vi.importActual<
    typeof import("../../lib/tauri-bridge")
  >("../../lib/tauri-bridge");
  return {
    ...actual,
    prepareCompare: (a: string, b: string) => prepareCompareMock(a, b),
    acceptB: (b: string) => acceptBMock(b),
    endCompare: (id: number) => endCompareMock(id),
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
  endCompareMock.mockReset();
  endCompareMock.mockResolvedValue(undefined);
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

  it("accepts through the parent's acceptB when it gives one, and not the bridge (#498)", async () => {
    // The parent puts the move in line with the session's other edits;
    // going round it would send the head move out of turn.
    prepareCompareMock.mockResolvedValue({ a_path: A_PATH, b_path: B_PATH });
    const acceptB = vi.fn().mockResolvedValue(B_NODE);
    const onAcceptB = vi.fn();

    render(
      <ABCompareBar
        aNodeId={A_NODE}
        bNodeId={B_NODE}
        onAudioPathChange={vi.fn()}
        acceptB={acceptB}
        onAcceptB={onAcceptB}
        onClose={vi.fn()}
      />,
    );
    await waitFor(() =>
      expect(screen.getByTestId("ab-accept-b")).not.toBeDisabled(),
    );
    await act(async () => {
      fireEvent.click(screen.getByTestId("ab-accept-b"));
    });

    expect(acceptB).toHaveBeenCalledWith(B_NODE);
    expect(acceptBMock).not.toHaveBeenCalled();
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

describe("ABCompareBar render warnings (#532)", () => {
  const reverb = {
    scope: { type: "track", index: 0, name: "Voice" },
    effect_index: 1,
    kind: "reverb",
    reason: "not_yet_streamable",
  };

  const bar = () => (
    <ABCompareBar
      aNodeId={A_NODE}
      bNodeId={B_NODE}
      onAudioPathChange={vi.fn()}
      onAcceptB={vi.fn()}
      onClose={vi.fn()}
    />
  );

  it("lists what the A side leaves out, and marks it A", async () => {
    prepareCompareMock.mockResolvedValue({
      a_path: A_PATH,
      b_path: B_PATH,
      a_warnings: [reverb],
      b_warnings: [],
    });

    render(bar());

    const notice = await screen.findByTestId("ab-render-warnings");
    expect(notice).toHaveTextContent('A · Reverb on track 0 "Voice"');
    expect(notice).not.toHaveTextContent("B ·");
  });

  it("says nothing when neither side leaves anything out", async () => {
    prepareCompareMock.mockResolvedValue({
      a_path: A_PATH,
      b_path: B_PATH,
      a_warnings: [],
      b_warnings: [],
    });

    render(bar());

    await waitFor(() => expect(screen.getByTestId("ab-side-a")).not.toBeDisabled());
    expect(screen.queryByTestId("ab-render-warnings")).not.toBeInTheDocument();
  });
});

/**
 * The backend keeps both renders out of the preview cache's eviction while
 * the bar is open, because every switch loads a side from its file again
 * (#575). The bar has to say when it is done with them, or the pair stays
 * held until the next compare.
 */
describe("ABCompareBar lets go of the renders it compared (#575)", () => {
  const prepared = (compare_id: number) => ({
    a_path: A_PATH,
    b_path: B_PATH,
    a_warnings: [],
    b_warnings: [],
    compare_id,
  });

  const bar = (bNodeId = B_NODE, onAudioPathChange = vi.fn()) => (
    <ABCompareBar
      aNodeId={A_NODE}
      bNodeId={bNodeId}
      onAudioPathChange={onAudioPathChange}
      onAcceptB={vi.fn()}
      onClose={vi.fn()}
    />
  );

  it("keeps the pair while the bar is open, and ends the compare when it closes", async () => {
    prepareCompareMock.mockResolvedValue(prepared(7));

    const { unmount } = render(bar());
    await waitFor(() => expect(screen.getByTestId("ab-side-b")).not.toBeDisabled());
    fireEvent.click(screen.getByTestId("ab-side-b"));
    fireEvent.click(screen.getByTestId("ab-side-a"));
    expect(endCompareMock).not.toHaveBeenCalled();

    unmount();
    expect(endCompareMock).toHaveBeenCalledTimes(1);
    expect(endCompareMock).toHaveBeenCalledWith(7);
  });

  it("ends a compare that finishes rendering after the bar has closed, and plays nothing", async () => {
    let finish: (value: ReturnType<typeof prepared>) => void = () => undefined;
    prepareCompareMock.mockReturnValue(
      new Promise((resolve) => {
        finish = resolve;
      }),
    );
    const onAudioPathChange = vi.fn();

    const { unmount } = render(bar(B_NODE, onAudioPathChange));
    unmount();
    expect(endCompareMock).not.toHaveBeenCalled();
    await act(async () => {
      finish(prepared(8));
    });

    await waitFor(() => expect(endCompareMock).toHaveBeenCalledWith(8));
    expect(onAudioPathChange).not.toHaveBeenCalled();
  });

  it("ends the old pair's compare when the bar moves to another pair", async () => {
    const C_NODE = "c".repeat(64);
    prepareCompareMock.mockResolvedValueOnce(prepared(1)).mockResolvedValueOnce(prepared(2));

    const { rerender, unmount } = render(bar(B_NODE));
    await waitFor(() => expect(screen.getByTestId("ab-side-b")).not.toBeDisabled());

    rerender(bar(C_NODE));
    expect(endCompareMock).toHaveBeenCalledWith(1);
    await waitFor(() => expect(prepareCompareMock).toHaveBeenLastCalledWith(A_NODE, C_NODE));
    await waitFor(() => expect(screen.getByTestId("ab-side-b")).not.toBeDisabled());
    expect(endCompareMock).toHaveBeenCalledTimes(1);

    unmount();
    expect(endCompareMock).toHaveBeenLastCalledWith(2);
  });

  it("does not switch to the old pair's renders once the new pair has failed to render", async () => {
    const C_NODE = "c".repeat(64);
    const onAudioPathChange = vi.fn();
    prepareCompareMock
      .mockResolvedValueOnce(prepared(1))
      .mockRejectedValueOnce(new Error("render failed"));

    const { rerender } = render(bar(B_NODE, onAudioPathChange));
    await waitFor(() => expect(onAudioPathChange).toHaveBeenCalledWith(A_PATH));
    rerender(bar(C_NODE, onAudioPathChange));
    await screen.findByTestId("ab-error");
    onAudioPathChange.mockClear();

    // The old pair's renders are no longer held, and are not C's.
    fireEvent.click(screen.getByTestId("ab-side-b"));
    expect(onAudioPathChange).not.toHaveBeenCalled();
  });

  it("an end the backend refuses is not shown", async () => {
    prepareCompareMock.mockResolvedValue(prepared(3));
    endCompareMock.mockRejectedValue("internal: a state mutex was poisoned (compare)");

    const { unmount } = render(bar());
    await waitFor(() => expect(screen.getByTestId("ab-side-b")).not.toBeDisabled());
    unmount();

    expect(endCompareMock).toHaveBeenCalledWith(3);
  });
});
