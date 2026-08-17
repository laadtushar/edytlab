/**
 * file-open — small helpers around the Tauri 2 file-picker dialog and
 * native drag-and-drop event.
 *
 * Centralised here so the toolbar button, the native `File > Open
 * Audio…` menu entry, and OS-level drag-and-drop all funnel through a
 * single "open this audio file" code path: `loadAudio(path)` →
 * setAudioPath + bridgeSendMessage("load this file: …"). That mirrors
 * the backend's expectation that loading a file is just another chat
 * message the agent picks up.
 */

import { open } from "@tauri-apps/plugin-dialog";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { sendMessage as bridgeSendMessage } from "./tauri-bridge";

const AUDIO_FILTERS = [
  {
    name: "Audio",
    extensions: ["wav", "mp3", "flac", "ogg", "m4a", "aac"],
  },
];

/**
 * Common load path: tell the agent the user just supplied a file at
 * `path`, then surface it to React state via `onLoaded`.
 *
 * Failures from `bridgeSendMessage` are passed back to the caller via
 * `onError`; the caller decides whether to show a toast / inline
 * error. We don't throw — the menu, button, and drag-drop callers are
 * fire-and-forget.
 */
export async function loadAudio(
  path: string,
  onLoaded: (path: string) => void,
  onError?: (err: string) => void,
): Promise<void> {
  onLoaded(path);
  try {
    await bridgeSendMessage(`load this file: ${path}`);
  } catch (err) {
    onError?.(String(err));
  }
}

/**
 * Show the OS file picker. Returns the absolute path the user chose,
 * or `null` if they cancelled. Multi-select is intentionally off —
 * the timeline currently loads a single mix.
 */
export async function pickAudioFile(): Promise<string | null> {
  const result = await open({
    multiple: false,
    directory: false,
    filters: AUDIO_FILTERS,
  });
  // Tauri 2 returns the path as `string | null` when `multiple: false`.
  if (typeof result === "string") return result;
  return null;
}

/**
 * Show the OS file picker with multi-select enabled. Returns an array of
 * absolute paths, or `null` if the user cancelled. When `multiple` is
 * `false` this is equivalent to `pickAudioFile` but wrapped in an array.
 */
export async function pickAudioFiles(
  multiple: boolean = true,
): Promise<string[] | null> {
  const result = await open({
    multiple,
    directory: false,
    filters: AUDIO_FILTERS,
  });
  if (!result) return null;
  return Array.isArray(result) ? result : [result];
}

/**
 * Show the OS picker for a *project* directory (#156).
 *
 * A project is a folder — the store lives in `.audiograph/` inside it —
 * so this is a directory picker rather than a file one. Returns the
 * absolute path, or `null` if the user cancelled.
 */
export async function pickProjectDirectory(): Promise<string | null> {
  const result = await open({ multiple: false, directory: true });
  return typeof result === "string" ? result : null;
}

/**
 * Subscribe to native (OS-level) drag-and-drop. Tauri 2's webview
 * intercepts file drops itself, so HTML5 `onDrop` never sees them on
 * Windows or Linux.
 *
 * The callback receives **every** dropped path. It used to receive only
 * `paths[0]`: dropping five files loaded one and discarded the rest
 * without saying so, which is the kind of silence that reads as a
 * broken drop rather than a deliberate limit.
 *
 * Returns an unlisten function. Safe to call outside a Tauri runtime
 * (e.g. in vitest / jsdom): in that case `getCurrentWebview()` throws
 * and we return a no-op unlisten so callers don't have to special-case
 * the test environment.
 */
export async function listenToFileDrops(
  onDropped: (paths: string[]) => void,
): Promise<() => void> {
  let webview;
  try {
    webview = getCurrentWebview();
  } catch {
    return () => undefined;
  }
  return await webview.onDragDropEvent((event) => {
    if (event.payload.type !== "drop") return;
    const paths = event.payload.paths ?? [];
    if (paths.length === 0) return;
    onDropped(paths);
  });
}
