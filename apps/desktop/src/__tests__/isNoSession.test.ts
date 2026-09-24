/**
 * `isNoSession` recognises the backend's own `NoSession` text (#341).
 *
 * Commands return `CmdResult<T>` = `Result<T, String>`, so the frontend
 * only ever sees the error's `Display`. Read from the Rust source rather
 * than copied, so rewording the message there cannot silently turn a new
 * project's empty track list back into an error banner.
 */

import { readFileSync } from "node:fs";
import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

import { isNoSession } from "../lib/tauri-bridge";

const commands = readFileSync(
  resolve(__dirname, "../../src-tauri/src/commands.rs"),
  "utf8",
);

describe("isNoSession", () => {
  it("recognises CommandError::NoSession as the backend words it", () => {
    const m = /#\[error\("([^"]+)"\)\]\s*NoSession/.exec(commands);
    expect(m, "CommandError::NoSession's #[error] attribute").not.toBeNull();
    expect(isNoSession(m![1])).toBe(true);
  });

  it("does not mistake other failures for it", () => {
    expect(isNoSession("permission denied: /home/user/Music/locked")).toBe(false);
    expect(isNoSession(new Error("load tool error: unsupported audio format"))).toBe(false);
  });
});
