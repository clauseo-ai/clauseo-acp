/// <reference types="vitest/config" />
import babel from "@rolldown/plugin-babel";
import react, { reactCompilerPreset } from "@vitejs/plugin-react";
import { defineConfig } from "vite";
import tauriConf from "../../crates/desktop/tauri.conf.json" with { type: "json" };

export default defineConfig({
  plugins: [react(), babel({ presets: [reactCompilerPreset()] })],
  server: {
    // Tauri's devUrl. The watcher stays out of the Rust crates and the multi-GB target folders.
    port: 1420,
    strictPort: true,
    watch: { ignored: ["**/crates/**", "**/target/**"] },
    // In dev the page comes from Vite, not Tauri's protocol, so Tauri sets no CSP. Serve the dev
    // policy from tauri.conf.json (the production one plus inline script and style for React
    // refresh and Vite's CSS, and Vite's websocket), so CSP and Trusted Types errors show while
    // building. The websocket allows any localhost port: `cargo xtask buildtime web` runs Vite on
    // a free port.
    headers: { "Content-Security-Policy": tauriConf.app.security.devCsp },
  },
  // WebView2 (Chromium), WKWebView (Safari 16+) and WebKitGTK all run ES2022.
  build: { target: "es2022" },
  test: {
    include: ["src/**/*.test.{ts,tsx}", "scripts/**/*.test.mjs"],
    environment: "node",
  },
});
