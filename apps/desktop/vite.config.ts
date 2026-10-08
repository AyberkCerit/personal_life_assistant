/// <reference types="vitest/config" />
import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

export default defineConfig({
  plugins: [svelte()],
  clearScreen: false,
  server: { port: 5173, strictPort: true },
  // css: true so `?raw` imports of stylesheets return their text (theme tests).
  test: { environment: "node", include: ["src/**/*.test.ts", "scripts/**/*.test.mjs"], css: true },
});
