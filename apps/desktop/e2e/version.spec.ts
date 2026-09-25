/**
 * The built app says which version it is.
 *
 * The status bar printed a literal `v0.1.0` of its own, so a release
 * would have gone out naming a version it was not. It now prints what
 * `tauri.conf.json` says, baked in when the frontend is built — and this
 * checks the bundle that ships, not a test's own reading of the file.
 */

import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import { projectWith, toneTrack } from "./backend";
import { expect, test } from "./fixtures";

const conf = JSON.parse(
  readFileSync(
    join(dirname(fileURLToPath(import.meta.url)), "../src-tauri/tauri.conf.json"),
    "utf8",
  ),
) as { version: string };

test("the status bar names the version in tauri.conf.json", async ({ app }) => {
  await app.boot(projectWith([toneTrack()], "1".padStart(64, "0")));
  await expect(app.page.getByTestId("status-bar-version")).toHaveText(`v${conf.version}`);
});
