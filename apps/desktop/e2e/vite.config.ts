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

import { createReadStream, statSync } from "node:fs";
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
        res.setHeader("Content-Length", size);
        createReadStream(requested).pipe(res);
      });
    },
  };
}

export default defineConfig(async (env) => {
  const base: UserConfig =
    typeof appConfig === "function" ? await appConfig(env) : await appConfig;
  return mergeConfig(base, {
    plugins: [serveFixtures()],
    build: {
      outDir: "e2e/.dist",
      emptyOutDir: true,
      rollupOptions: { input: { harness: "e2e/harness.html" } },
    },
  });
});
