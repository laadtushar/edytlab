/**
 * Ask a yes/no question before something that cannot be undone, and wait
 * for the answer. Use it for every destructive confirmation:
 *
 *     if (!(await confirmAction(`Delete skill "${name}"?`))) return;
 *
 * Never `if (!window.confirm(…)) return;` — see below.
 *
 * Why this is not just `window.confirm`
 * -------------------------------------
 * In the packaged app, tauri-plugin-dialog's init script replaces
 * `window.confirm` with an `async function`, so the "answer" is a
 * `Promise<boolean>` (#544). A Promise is always truthy, so a bare
 * `!window.confirm(…)` is always false: the delete ran at once and Cancel
 * was ignored. A browser, jsdom and Playwright all answer synchronously,
 * so no test could see it.
 *
 * Awaiting fixes the shape but not the second problem. The replacement
 * asks the plugin for a `confirm` command, and tauri-plugin-dialog 2.7.x
 * registers only `open`, `save` and `message` (2.7.0 folded `ask` and
 * `confirm` into `message`; `allow-confirm` is now an alias for
 * `allow-message`). Read from the crate source, not watched in a packaged
 * build: if it holds, `window.confirm` there is a Promise that rejects
 * and no dialog ever appears.
 *
 * What it does
 * ------------
 * - In the packaged app (`isTauri()`): the plugin's own `confirm()`, an
 *   OK/Cancel dialog sent as the `message` command. That command is the
 *   one `dialog:default` allows in `capabilities/default.json`.
 * - Anywhere else (browser, vitest/jsdom, Playwright): `window.confirm`,
 *   awaited. That keeps the existing stubs and Playwright's dialog
 *   handling working, and it still reads correctly if a webview's
 *   `confirm` answers with a Promise.
 *
 * The plugin's `confirm()` is chosen over trusting `window.confirm`
 * because it is the documented API and the path that has a command to
 * reach. If the dialog cannot be shown the promise rejects. A caller must
 * not read that as a yes, nor swallow it: report it (the editors show it
 * in their status line) and leave the thing alone.
 */

import { isTauri } from "@tauri-apps/api/core";
import { confirm as nativeConfirm } from "@tauri-apps/plugin-dialog";

export async function confirmAction(message: string): Promise<boolean> {
  if (isTauri()) {
    return nativeConfirm(message, { kind: "warning" });
  }
  return Boolean(await Promise.resolve(window.confirm(message)));
}
