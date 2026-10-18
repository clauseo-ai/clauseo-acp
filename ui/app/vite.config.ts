/// <reference types="vitest/config" />
import babel from "@rolldown/plugin-babel";
import react, { reactCompilerPreset } from "@vitejs/plugin-react";
import { defineConfig } from "vite";

export default defineConfig({
  plugins: [react(), babel({ presets: [reactCompilerPreset()] })],
  // Tauri's devUrl. The watcher stays out of the Rust crates and the multi-GB target folders.
  server: { port: 1420, strictPort: true, watch: { ignored: ["**/crates/**", "**/target/**"] } },
  // WebView2 (Chromium), WKWebView (Safari 16+) and WebKitGTK all run ES2022.
  build: { target: "es2022" },
  test: { include: ["src/**/*.test.{ts,tsx}"], environment: "node" },
});
