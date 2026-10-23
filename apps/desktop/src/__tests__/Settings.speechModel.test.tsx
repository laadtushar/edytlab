/**
 * Settings shows the speech model row beside the ONNX Runtime line (#383).
 *
 * In the panel, at the foot of the Account tab, after the runtime line.
 * Not in the first-run card (already as tall as a small window allows, and
 * of no use to someone who has not set a key). The row's states are
 * `SpeechModelRow.test.tsx`'s; this checks where it is mounted and that
 * opening Settings asks for the model's status and downloads nothing.
 */

import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mlModelStatusMock = vi.fn();
const downloadMock = vi.fn();

vi.mock("../lib/tauri-bridge", () => ({
  mlRuntimeStatus: vi.fn().mockResolvedValue({ state: "missing", searched: [] }),
  mlModelStatus: (model: string) => mlModelStatusMock(model),
  onMlModelProgress: () => Promise.resolve(() => undefined),
  downloadMlModel: (model: string) => downloadMock(model),
  cancelMlModelDownload: vi.fn(),
  removeMlModel: vi.fn(),
  getActiveProvider: vi.fn().mockResolvedValue(""),
  getActiveModel: vi.fn().mockResolvedValue(""),
  getBaseUrlFor: vi.fn().mockResolvedValue(null),
  defaultBaseUrlFor: vi.fn().mockResolvedValue("https://api.anthropic.com"),
  getEffortFor: vi.fn().mockResolvedValue(null),
  setEffortFor: vi.fn(),
  listModelsFor: vi.fn().mockResolvedValue({ models: [], source: "live", notice: null }),
  hasApiKeyFor: vi.fn().mockResolvedValue(true),
  clearApiKeyFor: vi.fn(),
  setApiKeyFor: vi.fn(),
  setActiveProvider: vi.fn(),
  setActiveModel: vi.fn(),
  setBaseUrlFor: vi.fn(),
  testApiKeyFor: vi.fn(),
  installPlugin: vi.fn(),
}));

import { Settings } from "../components/Settings";

describe("Settings: the speech model row", () => {
  beforeEach(() => {
    mlModelStatusMock.mockReset().mockResolvedValue({
      model: "whisper-base",
      state: "not_downloaded",
      bytes: 0,
      size: 138_644_904,
    });
    downloadMock.mockReset();
    window.localStorage.clear();
  });

  it("sits under the ONNX Runtime line in the panel's Account tab", async () => {
    render(<Settings mode="panel" onSaved={vi.fn()} onClose={vi.fn()} />);

    const runtime = await screen.findByTestId("settings-ml-runtime");
    const model = await screen.findByTestId("settings-speech-model");
    // The row follows the line in the document.
    expect(
      runtime.compareDocumentPosition(model) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
    expect(await screen.findByTestId("speech-model-download")).toHaveTextContent("139 MB");
  });

  it("asks for the status on opening and downloads nothing", async () => {
    render(<Settings mode="panel" onSaved={vi.fn()} onClose={vi.fn()} />);
    await screen.findByTestId("speech-model-download");

    expect(mlModelStatusMock).toHaveBeenCalledWith("whisper-base");
    expect(downloadMock).not.toHaveBeenCalled();
  });

  it("is on the Account tab only", async () => {
    const user = userEvent.setup();
    render(<Settings mode="panel" onSaved={vi.fn()} onClose={vi.fn()} />);
    await screen.findByTestId("settings-speech-model");

    await user.click(screen.getByTestId("settings-tab-plugins"));
    expect(screen.queryByTestId("settings-speech-model")).toBeNull();
    expect(downloadMock).not.toHaveBeenCalled();
  });

  it("is left out of the first-run card, which does not ask for it", async () => {
    render(<Settings mode="blocking" onSaved={vi.fn()} />);
    await screen.findByTestId("settings-provider-picker");

    expect(screen.queryByTestId("settings-speech-model")).toBeNull();
    expect(mlModelStatusMock).not.toHaveBeenCalled();
    expect(downloadMock).not.toHaveBeenCalled();
  });

  /**
   * Open Settings on the speech-setup card has to land on the row (#384). The
   * row is the last thing on a tall Account tab, so opening the tab at its top
   * left it below the fold, and the card said "press Download beside it".
   * jsdom has no layout: the boxes are stood in for, and what is checked is
   * that the body, and only the body, is scrolled to put the row near its top.
   */
  describe("revealing the row", () => {
    function boxes(rowTop: number, bodyTop: number) {
      return vi
        .spyOn(Element.prototype, "getBoundingClientRect")
        .mockImplementation(function (this: Element) {
          const id = this.getAttribute("data-testid");
          const top = id === "settings-speech-model" ? rowTop : id === "settings-body" ? bodyTop : 0;
          return { top, bottom: top + 100, left: 0, right: 100, width: 100, height: 100, x: 0, y: top, toJSON: () => ({}) };
        });
    }

    it("scrolls the body to the row when asked to reveal it", async () => {
      const spy = boxes(900, 100);
      try {
        render(<Settings mode="panel" reveal="speech-model" onSaved={vi.fn()} onClose={vi.fn()} />);
        await screen.findByTestId("speech-model-download");
        const body = screen.getByTestId("settings-body");
        // The row is 800 px below the body's top: it is brought to near the top.
        await vi.waitFor(() => expect(body.scrollTop).toBeGreaterThanOrEqual(780));
        expect(body.scrollTop).toBeLessThanOrEqual(800);
      } finally {
        spy.mockRestore();
      }
    });

    it("opens at the top, as before, when nothing asks for the row", async () => {
      const spy = boxes(900, 100);
      try {
        render(<Settings mode="panel" onSaved={vi.fn()} onClose={vi.fn()} />);
        await screen.findByTestId("speech-model-download");
        expect(screen.getByTestId("settings-body").scrollTop).toBe(0);
      } finally {
        spy.mockRestore();
      }
    });
  });
});
