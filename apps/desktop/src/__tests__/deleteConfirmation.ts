/**
 * The contract every destructive "Delete" button shares: nothing is
 * deleted until the person has answered, and only "OK" deletes.
 *
 * `window.confirm` does not answer in one shape. A browser, jsdom and
 * Playwright return a boolean. In the packaged app, tauri-plugin-dialog's
 * init script replaces it with an `async function`, so the answer is a
 * `Promise<boolean>` (#544). A Promise is truthy: `if (!window.confirm(…))
 * return;` never returns, the delete runs at once, and Cancel is ignored.
 * No test saw it, because every test stubbed `confirm` with a plain
 * `true`. This runs each editor's Delete against every shape.
 *
 * Shared by the profile, skill and MCP server editors, which differ only
 * in how they open an entry and which bridge call Delete ends in.
 */

import { act, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { held } from "./held";

/** The shapes an answer to `window.confirm` can take, and what each means. */
export const CONFIRM_SHAPES: ReadonlyArray<{
  name: string;
  answer: () => unknown;
  confirmed: boolean;
}> = [
  {
    name: "a synchronous false (browser, jsdom, Playwright)",
    answer: () => false,
    confirmed: false,
  },
  {
    name: "a synchronous true (browser, jsdom, Playwright)",
    answer: () => true,
    confirmed: true,
  },
  {
    name: "Promise.resolve(false), as tauri-plugin-dialog returns for Cancel",
    answer: () => Promise.resolve(false),
    confirmed: false,
  },
  {
    name: "Promise.resolve(true), as tauri-plugin-dialog returns for OK",
    answer: () => Promise.resolve(true),
    confirmed: true,
  },
];

/** A bridge mock, seen only as far as these tests need. */
type CallSpy = { mock: { calls: unknown[][] } };

export interface DeleteHarness {
  /** Render the editor, open its one saved entry, return the Delete button. */
  openDeleteButton: () => Promise<HTMLElement>;
  /** The bridge call a confirmed Delete ends in. */
  deleteCommand: CallSpy;
  /** What that call receives for the entry `openDeleteButton` opened. */
  deleteArgs: unknown[];
  /** The test id of the line the editor reports failures on. */
  statusTestId: string;
}

function stubConfirm(answer: unknown) {
  // `spyOn` on a method an earlier test already spied returns that same
  // spy, calls and all, so clear it.
  return vi
    .spyOn(window, "confirm")
    .mockClear()
    .mockReturnValue(answer as boolean);
}

/**
 * Let every promise continuation settle. A Delete that was going to run
 * after the answer does so in microtasks, so one macrotask later it has
 * run, however late React's scheduler is: that is what makes "was not
 * called" a real assertion and not an early look.
 */
async function settle() {
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
}

export function describeDeleteConfirmation(name: string, h: DeleteHarness) {
  describe(`${name}: Delete waits for the confirmation`, () => {
    it.each(CONFIRM_SHAPES)(
      "window.confirm answers $name",
      async ({ answer, confirmed }) => {
        const user = userEvent.setup();
        const confirm = stubConfirm(answer());
        const del = await h.openDeleteButton();

        await user.click(del);
        await settle();

        expect(confirm).toHaveBeenCalledTimes(1);
        if (confirmed) {
          await waitFor(() =>
            expect(h.deleteCommand).toHaveBeenCalledWith(...h.deleteArgs),
          );
        } else {
          expect(
            h.deleteCommand.mock.calls,
            "Cancel was ignored: Delete ran although the answer was no",
          ).toHaveLength(0);
        }
      },
    );

    it("deletes nothing while the dialog is still open, then nothing on Cancel", async () => {
      const user = userEvent.setup();
      const answer = held<boolean>();
      stubConfirm(answer.promise);
      const del = await h.openDeleteButton();

      await user.click(del);
      await settle();
      expect(
        h.deleteCommand.mock.calls,
        "Delete ran while the dialog was still open: the Promise was " +
          "treated as an answer",
      ).toHaveLength(0);

      await answer.resolve(false);
      await settle();
      expect(h.deleteCommand.mock.calls).toHaveLength(0);
    });

    it("reports it, and deletes nothing, when the dialog cannot be shown", async () => {
      const user = userEvent.setup();
      const answer = held<boolean>();
      stubConfirm(answer.promise);
      const del = await h.openDeleteButton();

      await user.click(del);
      await answer.reject("the dialog could not be opened");

      await waitFor(() =>
        expect(screen.getByTestId(h.statusTestId)).toHaveTextContent(
          "the dialog could not be opened",
        ),
      );
      expect(h.deleteCommand.mock.calls).toHaveLength(0);
    });

    it("deletes once the dialog is answered OK, and not before", async () => {
      const user = userEvent.setup();
      const answer = held<boolean>();
      stubConfirm(answer.promise);
      const del = await h.openDeleteButton();

      await user.click(del);
      await settle();
      expect(h.deleteCommand.mock.calls).toHaveLength(0);

      await answer.resolve(true);
      await waitFor(() =>
        expect(h.deleteCommand).toHaveBeenCalledWith(...h.deleteArgs),
      );
      expect(h.deleteCommand.mock.calls).toHaveLength(1);
    });
  });
}
