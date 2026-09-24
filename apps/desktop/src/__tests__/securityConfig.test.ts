/**
 * The webview's two security settings may not drift back (#334).
 *
 * `csp: null` let the webview run and fetch anything, and an asset scope
 * of `**` let it read any file the user can read — so the first injection
 * in a chat UI that renders model output would have been local file read
 * plus exfiltration. The policy was verified in the real WebKitGTK
 * webview (waveform, preview, compare, spectrogram worker, fonts all
 * working; a remote fetch and `/etc/hostname` over `asset:` refused), and
 * every e2e test runs under it. This pins the parts that make it a policy.
 */

import { readFileSync } from "node:fs";
import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

const conf = JSON.parse(
  readFileSync(resolve(__dirname, "../../src-tauri/tauri.conf.json"), "utf8"),
) as {
  app: {
    security: {
      csp: Record<string, string> | string | null;
      assetProtocol: { enable: boolean; scope: string[] };
    };
  };
};
const { csp, assetProtocol } = conf.app.security;

function sources(directive: string): string[] {
  if (!csp || typeof csp === "string") return [];
  return (csp[directive] ?? "").split(/\s+/).filter(Boolean);
}

describe("the shipped Content Security Policy", () => {
  it("exists", () => {
    expect(csp).not.toBeNull();
    expect(typeof csp).toBe("object");
  });

  it("defaults to the app's own origin", () => {
    expect(sources("default-src")).toEqual(["'self'"]);
  });

  it("runs only the app's own scripts", () => {
    const script = sources("script-src");
    expect(script).toContain("'self'");
    for (const unsafe of ["'unsafe-inline'", "'unsafe-eval'", "*", "http:", "https:", "data:", "blob:"]) {
      expect(script, `script-src must not allow ${unsafe}`).not.toContain(unsafe);
    }
  });

  it("connects only to the app, its IPC and its assets — nowhere remote", () => {
    for (const source of sources("connect-src")) {
      const local =
        source === "'self'" ||
        source === "ipc:" ||
        source === "asset:" ||
        source === "http://ipc.localhost" ||
        source === "http://asset.localhost";
      expect(local, `connect-src allows ${source}`).toBe(true);
    }
  });

  it("allows no plugins, and no rebasing or form posts", () => {
    expect(sources("object-src")).toEqual(["'none'"]);
    expect(sources("base-uri")).toEqual(["'self'"]);
    expect(sources("form-action")).toEqual(["'none'"]);
  });
});

describe("the asset protocol's scope", () => {
  it("is the app's own data directory, not the whole disk", () => {
    expect(assetProtocol.enable).toBe(true);
    // Everything else — projects, track sources, compare renders — is
    // granted at runtime, one directory or file at a time.
    expect(assetProtocol.scope).toEqual(["$APPDATA/**"]);
  });
});
