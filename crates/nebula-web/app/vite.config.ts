import { defineConfig } from "vite";
import { mockApi } from "./src/mock/plugin.ts";

export default defineConfig({
  plugins: [mockApi()],
  // One bundle, embedded in the binary and served from this Mac or over the
  // tailnet, cached for good once read: nothing gains from splitting it.
  build: { chunkSizeWarningLimit: 700 },
});
