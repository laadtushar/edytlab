/**
 * The harness, built the way the app ships: a production bundle.
 *
 * It used to run on the Vite dev server, which serves React's
 * development build. That is not the code that ships. Among other
 * differences, `<React.StrictMode>` runs every effect twice in
 * development, so every boot command was called twice, and a bug that
 * depends on an effect running once could not show.
 *
 * So this extends the app's own config, adds the harness page as the
 * only entry, builds it in production mode, and serves the result with
 * `vite preview`.
 */

import { createReadStream, readFileSync, statSync } from "node:fs";
import { resolve, sep } from "node:path";
import { defineConfig, mergeConfig, type Plugin, type UserConfig } from "vite";

import appConfig from "../vite.config";
import { FIXTURE_DIR } from "./audio-fixtures";
import { FILE_ROUTE } from "./routes";

/**
 * Tauri's asset protocol, stood in for.
 *
 * Tauri encodes a path as `asset://localhost/${encodeURIComponent(path)}`
 * (see `mockConvertFileSrc` in `@tauri-apps/api/mocks`), and `FILE_ROUTE`
 * takes the same encoding, so a Windows path (`C:\…`) and a POSIX one
 * travel the same way. The first version of this harness used Vite's
 * `/@fs` prefix, which a Windows path does not fit.
 *
 * Serve generated fixtures, and nothing else. The route takes an
 * absolute path, so it is confined to the fixture directory rather
 * than handing the page the whole disk, which is the mistake #334
 * describes in the app's own asset scope.
 */
function serveFixtures(): Plugin {
  const root = resolve(FIXTURE_DIR) + sep;
  return {
    name: "e2e-serve-fixtures",
    configurePreviewServer(server) {
      server.middlewares.use((req, res, next) => {
        if (!req.url?.startsWith(FILE_ROUTE)) return next();
        const requested = resolve(decodeURIComponent(req.url.slice(FILE_ROUTE.length)));
        if (!requested.startsWith(root)) {
          res.statusCode = 403;
          res.end("outside the fixture directory");
          return;
        }
        let size: number;
        try {
          size = statSync(requested).size;
        } catch {
          res.statusCode = 404;
          res.end("no such fixture");
          return;
        }
        // What Tauri's asset protocol answers (`tauri/src/protocol/
        // asset.rs`). The type matters more than it looks: WaveSurfer
        // plays from a blob only when the browser can play the blob's
        // type, and falls back to the bare URL otherwise. Sent without
        // one, the harness pushed every player onto the URL, where a
        // seek needs byte ranges, and a paused seek snapped back to 0 —
        // a failure the app, which gets `audio/x-wav` from `infer`'s
        // magic-byte sniffing, never has. Every fixture is a WAV.
        res.setHeader("Content-Type", "audio/x-wav");
        const range = /^bytes=(\d*)-(\d*)$/.exec(req.headers.range ?? "");
        if (!req.headers.range) {
          res.setHeader("Content-Length", size);
          createReadStream(requested).pipe(res);
          return;
        }
        // One range, as Tauri serves, capped at its 1000 KiB per reply.
        res.setHeader("Accept-Ranges", "bytes");
        const start = range?.[1] ? Number(range[1]) : range?.[2] ? size - Number(range[2]) : 0;
        const asked = range?.[1] && range[2] ? Number(range[2]) : size - 1;
        if (!range || start >= size || asked < start) {
          res.statusCode = 416;
          res.setHeader("Content-Range", `bytes */${size}`);
          res.end();
          return;
        }
        const end = Math.min(asked, size - 1, start + 1000 * 1024 - 1);
        res.statusCode = 206;
        res.setHeader("Content-Range", `bytes ${start}-${end}/${size}`);
        res.setHeader("Content-Length", end + 1 - start);
        createReadStream(requested, { start, end }).pipe(res);
      });
    },
  };
}

/**
 * The Content Security Policy the app ships (`tauri.conf.json`), as the
 * one header string Tauri sends with it (#334).
 *
 * Put on the harness page so every e2e test runs the frontend under the
 * policy the release enforces: a change that needs something the policy
 * forbids — an inline script, a remote fetch, a new worker source — fails
 * here, in CI, rather than as a silent break in a signed build. Tauri's
 * `asset:`/`ipc:` sources are inert in Chromium; the harness serves audio
 * and IPC from its own origin, which `'self'` covers.
 */
export function shippedCsp(): string {
  const conf = JSON.parse(
    readFileSync(new URL("../src-tauri/tauri.conf.json", import.meta.url), "utf8"),
  ) as { app: { security: { csp: Record<string, string> | null } } };
  const csp = conf.app.security.csp;
  if (!csp) throw new Error("tauri.conf.json ships no CSP");
  return Object.entries(csp)
    .map(([directive, sources]) => `${directive} ${sources}`)
    .join("; ");
}

function withShippedCsp(): Plugin {
  return {
    name: "e2e-shipped-csp",
    transformIndexHtml: (html) =>
      html.replace(
        "<head>",
        `<head>\n    <meta http-equiv="Content-Security-Policy" content="${shippedCsp()}" />`,
      ),
  };
}

export default defineConfig(async (env) => {
  const base: UserConfig =
    typeof appConfig === "function" ? await appConfig(env) : await appConfig;
  return mergeConfig(base, {
    plugins: [serveFixtures(), withShippedCsp()],
    build: {
      outDir: "e2e/.dist",
      emptyOutDir: true,
      rollupOptions: { input: { harness: "e2e/harness.html" } },
    },
  });
});
