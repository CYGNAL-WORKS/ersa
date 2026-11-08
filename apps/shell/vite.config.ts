import { resolve } from "node:path";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";

// The window loads index.html. gallery.html is the development-only gallery
// (ADR 0008): the dev server serves it, and the production build leaves it out
// because only index.html is an input.
export default defineConfig({
  plugins: [react()],
  // Tauri expects a fixed port and shows its own errors, so Vite should not
  // pick another port or clear the terminal.
  server: { port: 5173, strictPort: true },
  clearScreen: false,
  build: {
    rollupOptions: { input: resolve(import.meta.dirname, "index.html") },
  },
  test: {
    include: ["src/**/*.test.ts"],
    environment: "node",
    // Not UTC, which CI's runners default to: there a date read in UTC instead of local time would go unnoticed
    // (FLOW-ORGANIZE-SESSIONS-R07).
    env: { TZ: "America/New_York" },
  },
});
