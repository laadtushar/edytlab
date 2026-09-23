/// <reference types="vitest" />
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

// No `@ts-expect-error` here any more. It claimed `process` was
// untyped, but `@types/node` is installed and picked up implicitly, so
// the directive itself was the error: `tsc -p tsconfig.node.json`
// failed with "Unused '@ts-expect-error' directive" and nobody saw,
// because nothing type-checked this file. The e2e harness imports it,
// and `tsc -p e2e` is what found it.
const host = process.env.TAURI_DEV_HOST;

// https://vite.dev/config/
export default defineConfig(async () => ({
  plugins: [react(), tailwindcss()],

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
