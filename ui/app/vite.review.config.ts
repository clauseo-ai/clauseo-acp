/// <reference types="vitest/config" />
// The dev-only copy review (review.html, src/review/): `pnpm build:review` builds it into dist-review/ as static
// files with relative URLs, so it works served from any path; `pnpm copy:check` runs its release gate. The app's
// build (vite.config.ts) never takes review.html as an input, so nothing here ships.
import babel from "@rolldown/plugin-babel";
import tailwindcss from "@tailwindcss/vite";
import react, { reactCompilerPreset } from "@vitejs/plugin-react";
import { defineConfig } from "vite";
import { cspMeta } from "./vite.config.ts";

export default defineConfig({
  plugins: [tailwindcss(), react(), babel({ presets: [reactCompilerPreset()] }), cspMeta()],
  base: "./",
  build: {
    target: "es2022",
    assetsInlineLimit: 0,
    outDir: "dist-review",
    emptyOutDir: true,
    rollupOptions: { input: { review: "review.html" } },
  },
  test: {
    // only the gate: `pnpm test` runs the status logic's unit tests, never this
    include: ["src/review/*.check.ts"],
    environment: "node",
  },
});
