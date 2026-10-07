// Driving what WebDriver cannot see: native windows such as the GTK file
// chooser, through the X server, the way a person uses the keyboard.
//
// The page cannot stub these (`__TAURI_INTERNALS__` is locked down), so
// stories go through the real dialog.
import { execFileSync } from "node:child_process";
import { copyFileSync, mkdirSync, mkdtempSync } from "node:fs";
import { basename, join } from "node:path";

const env = { ...process.env, DISPLAY: process.env.DISPLAY ?? ":99" };
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

export function x(...args) {
  // stderr is dropped: a window closing mid-search prints a BadWindow
  // error that means nothing here.
  return execFileSync("xdotool", args, { env, stdio: ["ignore", "pipe", "ignore"] }).toString().trim();
}

/** Names of the visible top-level windows. */
export function windows() {
  let ids = [];
  try {
    ids = x("search", "--onlyvisible", "--name", ".").split("\n").filter(Boolean);
  } catch {
    return [];
  }
  return ids.map((id) => {
    let name = "";
    try {
      name = x("getwindowname", id);
    } catch {}
    return { id, name };
  });
}

export async function waitForWindow(pattern, timeout = 15000) {
  const end = Date.now() + timeout;
  while (Date.now() < end) {
    const w = windows().find((w) => pattern.test(w.name));
    if (w) return w;
    await sleep(200);
  }
  throw new Error(`no native window matching ${pattern} (have: ${windows().map((w) => w.name).join(", ")})`);
}

export async function waitForNoWindow(pattern, timeout = 15000) {
  const end = Date.now() + timeout;
  while (Date.now() < end) {
    if (!windows().some((w) => pattern.test(w.name))) return;
    await sleep(200);
  }
  throw new Error(`native window ${pattern} did not close`);
}

/** A screenshot of the whole screen, native dialogs included. */
export function screenshotRoot(file) {
  execFileSync("import", ["-window", "root", file], { env });
}

/**
 * Click `selector` (which opens the GTK chooser) and answer it:
 *  - `{ path }`   type one path into the location bar (Ctrl+L) and accept
 *  - `{ paths }`  several files: they are copied alone into a new folder,
 *                 which is opened and everything in it selected (Ctrl+A)
 *  - `{ cancel }` press Escape
 * `shotDialog(caption)` is called while the chooser is on screen.
 */
export async function chooseThrough(d, selector, answer, { shotDialog } = {}) {
  // The click returns when the dialog closes, so it is not awaited yet.
  const clicked = d.click(selector).catch((e) => e);
  const title = /open|select|save|choose/i;
  const dialog = await waitForWindow(title);
  await sleep(700);
  if (shotDialog) await shotDialog();
  if (answer.cancel) {
    x("key", "Escape");
  } else if (answer.paths) {
    const dir = mkdtempSync("/tmp/edytlab-pick-");
    for (const p of answer.paths) copyFileSync(p, join(dir, basename(p)));
    x("key", "ctrl+l");
    await sleep(300);
    x("type", "--delay", "15", `${dir}/`);
    await sleep(500);
    // The location bar completes the common prefix of what the folder
    // holds ("t" for tone and take2) as selected text; Return would then
    // accept "<dir>/t". Deleting the selection leaves the folder itself.
    x("key", "Delete");
    await sleep(200);
    x("key", "Return");
    await sleep(800);
    x("key", "ctrl+a");
    await sleep(300);
    x("key", "Return");
  } else {
    // A folder chooser opens on "Recent", which cannot be chosen: Return
    // on a typed path only walks into the folder and Open stays greyed
    // out. Going to the home folder first (GTK's Alt+Home) puts it on a
    // real location, where Return on the typed path selects the folder.
    if (/folder/i.test(dialog.name)) {
      x("key", "alt+Home");
      await sleep(600);
    }
    x("key", "ctrl+l");
    await sleep(300);
    x("type", "--delay", "15", answer.path);
    await sleep(400);
    x("key", "Return");
  }
  await waitForNoWindow(title);
  const outcome = await clicked;
  if (outcome instanceof Error) throw outcome;
}
