/// <reference types="vitest" />
import { readFileSync } from "node:fs";

import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import license from "rollup-plugin-license";

// No `@ts-expect-error` here any more. It claimed `process` was
// untyped, but `@types/node` is installed and picked up implicitly, so
// the directive itself was the error: `tsc -p tsconfig.node.json`
// failed with "Unused '@ts-expect-error' directive" and nobody saw,
// because nothing type-checked this file. The e2e harness imports it,
// and `tsc -p e2e` is what found it.
const host = process.env.TAURI_DEV_HOST;

/**
 * The app's version, read from the one place it is set. `tauri.conf.json`
 * is canonical (see CLAUDE.md); the status bar used to print a literal
 * `v0.1.0` of its own, which nothing would have updated on a release.
 */
const appVersion: string = JSON.parse(
  readFileSync(new URL("./src-tauri/tauri.conf.json", import.meta.url), "utf8"),
).version;

/**
 * Third-party notices (scripts/third-party-notices/generate.mjs). When this
 * variable names a file, the production build also writes the packages that
 * ended up in the JavaScript bundle there, with their licence and NOTICE
 * texts, as JSON. Unset (every normal build, `tauri build` included) the
 * plugin is not even loaded, so the build is exactly what it was.
 */
const noticesJson = process.env.EDYTLAB_NOTICES_JSON;

// https://vite.dev/config/
export default defineConfig(async () => ({
  plugins: [
    react(),
    tailwindcss(),
    ...(noticesJson
      ? [
          license({
            thirdParty: {
              includePrivate: false,
              output: {
                file: noticesJson,
                template: (dependencies) =>
                  JSON.stringify(
                    dependencies.map((d) => ({
                      name: d.name,
                      version: d.version,
                      license: d.license,
                      licenseText: d.licenseText,
                      noticeText: d.noticeText,
                      repository:
                        typeof d.repository === "string"
                          ? d.repository
                          : d.repository?.url,
                      homepage: d.homepage,
                      author: d.author?.text(),
                    })),
                    null,
                    2,
                  ),
              },
            },
          }),
        ]
      : []),
  ],

  define: {
    __APP_VERSION__: JSON.stringify(appVersion),
  },

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // 3. tell Vite to ignore watching `src-tauri`
      ignored: ["**/src-tauri/**"],
    },
  },

  // Vitest configuration: jsdom environment for component tests, with
  // a setup file that wires up `@testing-library/jest-dom` matchers and
  // stubs the Tauri IPC layer so component code can import the bridge
  // safely (without a running webview behind it).
  test: {
    environment: "jsdom",
    globals: true,
    setupFiles: ["./src/__tests__/setup.ts"],
    css: false,
    include: ["src/**/*.{test,spec}.{ts,tsx}"],
  },
}));
