import { fileURLToPath } from "node:url";

import { defineConfig } from "vitest/config";

// The same `@/` alias tsconfig and Next use, so a test can import a
// module (the sitemap, say) that imports others by that path.
export default defineConfig({
  resolve: {
    alias: { "@": fileURLToPath(new URL(".", import.meta.url)) },
  },
});
