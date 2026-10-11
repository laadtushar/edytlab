/**
 * `confirmAction` — one awaited yes/no for every destructive button.
 *
 * Outside Tauri the question goes to `window.confirm`, whatever shape its
 * answer takes. Inside the packaged app it goes to the dialog plugin's
 * `message` command instead: the plugin's replacement `window.confirm`
 * asks for a `confirm` command that tauri-plugin-dialog 2.7.1 no longer
 * registers (see the doc comment on `confirmAction`).
 */

import { afterEach, beforeEach, describe, expect, it, vi, type Mock } from "vitest";
import { CONFIRM_SHAPES } from "../../__tests__/deleteConfirmation";
import { confirmAction } from "../confirmAction";

const internals = (
  window as unknown as { __TAURI_INTERNALS__: { invoke: Mock } }
).__TAURI_INTERNALS__;

function setIsTauri(on: boolean) {
  const g = globalThis as { isTauri?: boolean };
  if (on) g.isTauri = true;
  else delete g.isTauri;
}

beforeEach(() => {
  internals.invoke.mockReset().mockResolvedValue(undefined);
});

afterEach(() => {
  setIsTauri(false);
});

describe("confirmAction outside Tauri (browser, jsdom, Playwright)", () => {
  it.each(CONFIRM_SHAPES)(
    "reads the answer from $name",
    async ({ answer, confirmed }) => {
      vi.spyOn(window, "confirm").mockReturnValue(answer() as boolean);
      await expect(confirmAction("Delete it?")).resolves.toBe(confirmed);
    },
  );

  it("asks window.confirm the message it was given", async () => {
    const confirm = vi.spyOn(window, "confirm").mockReturnValue(true);
    await confirmAction("Delete profile \"x\"? This cannot be undone.");
    expect(confirm).toHaveBeenCalledWith(
      "Delete profile \"x\"? This cannot be undone.",
    );
  });

  it("does not resolve before a pending answer does", async () => {
    let answer!: (ok: boolean) => void;
    vi.spyOn(window, "confirm").mockReturnValue(
      new Promise<boolean>((resolve) => {
        answer = resolve;
      }) as unknown as boolean,
    );
    let settled: boolean | undefined;
    const pending = confirmAction("Delete it?").then((ok) => {
      settled = ok;
      return ok;
    });

    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(settled, "resolved on a Promise that nobody had answered").toBeUndefined();

    answer(false);
    await expect(pending).resolves.toBe(false);
  });

  it("treats a missing answer as no", async () => {
    // jsdom's own `confirm` is not implemented and returns undefined.
    vi.spyOn(window, "confirm").mockReturnValue(undefined as unknown as boolean);
    await expect(confirmAction("Delete it?")).resolves.toBe(false);
  });
});

describe("confirmAction inside the packaged app", () => {
  beforeEach(() => setIsTauri(true));

  it("asks the dialog plugin's message command, not window.confirm", async () => {
    const confirm = vi.spyOn(window, "confirm").mockClear();
    internals.invoke.mockResolvedValue("Ok");

    await confirmAction("Delete skill \"mix\"? This cannot be undone.");

    expect(confirm).not.toHaveBeenCalled();
    expect(internals.invoke).toHaveBeenCalledTimes(1);
    const [cmd, args] = internals.invoke.mock.calls[0];
    expect(cmd).toBe("plugin:dialog|message");
    expect(args).toMatchObject({
      message: "Delete skill \"mix\"? This cannot be undone.",
      buttons: "OkCancel",
      kind: "warning",
    });
  });

  it("never asks for the `confirm` command the plugin no longer registers", async () => {
    internals.invoke.mockResolvedValue("Ok");
    await confirmAction("Delete it?");
    const commands = internals.invoke.mock.calls.map(([cmd]) => cmd);
    expect(commands).not.toContain("plugin:dialog|confirm");
  });

  it.each([
    ["Ok", true],
    ["Cancel", false],
  ])("answers %s as %s", async (button, confirmed) => {
    internals.invoke.mockResolvedValue(button);
    await expect(confirmAction("Delete it?")).resolves.toBe(confirmed);
  });

  it("rejects, rather than guessing yes, when the dialog cannot be shown", async () => {
    internals.invoke.mockRejectedValue("dialog.message not allowed");
    await expect(confirmAction("Delete it?")).rejects.toBe(
      "dialog.message not allowed",
    );
  });
});
