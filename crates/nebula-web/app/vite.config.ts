import { defineConfig } from "vite";
import { mockApi } from "./src/mock/plugin.ts";

export default defineConfig({ plugins: [mockApi()] });
