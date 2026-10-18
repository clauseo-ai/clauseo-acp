/// <reference types="vitest/config" />
import babel from "@rolldown/plugin-babel";
import react, { reactCompilerPreset } from "@vitejs/plugin-react";
import { defineConfig } from "vite";
import tauriConf from "../../crates/desktop/tauri.conf.json" with { type: "json" };

/** The directives the dev policy loosens, and what each gets. */
const DEV_ADDITIONS: Record<string, string> = {
  // React Refresh's preamble is an inline script.
  "script-src": "'unsafe-inline'",
  // Vite injects CSS as <style> elements in dev.
  "style-src": "'unsafe-inline'",
  // Vite's HMR websocket. Any localhost port: `cargo xtask buildtime web` runs Vite on a free one.
  "connect-src": "ws://localhost:*",
};

/**
 * The dev CSP: the production `csp` from tauri.conf.json plus `DEV_ADDITIONS`, everything else
 * (Trusted Types included) unchanged. Built here because in dev the page comes from Vite, not
 * Tauri's protocol, so Tauri sets no CSP header and Vite is the only one that can.
 */
export function devCsp(csp: string): string {
  const directives = csp
    .split(";")
    .map((directive) => directive.trim())
    .filter((directive) => directive.length > 0);
  for (const name of Object.keys(DEV_ADDITIONS)) {
    if (!directives.some((directive) => directive.split(/\s+/)[0] === name)) {
      throw new Error(`tauri.conf.json app.security.csp has no ${name}; the dev CSP adds to it`);
    }
  }
  return directives
    .map((directive) => {
      const addition = DEV_ADDITIONS[directive.split(/\s+/)[0] ?? ""];
      return addition === undefined ? directive : `${directive} ${addition}`;
    })
    .join("; ");
}

export default defineConfig({
  plugins: [react(), babel({ presets: [reactCompilerPreset()] })],
  server: {
    // Tauri's devUrl. The watcher stays out of the Rust crates and the multi-GB target folders.
    port: 1420,
    strictPort: true,
    watch: { ignored: ["**/crates/**", "**/target/**"] },
    headers: { "Content-Security-Policy": devCsp(tauriConf.app.security.csp) },
  },
  // WebView2 (Chromium), WKWebView (Safari 16+) and WebKitGTK all run ES2022.
  build: { target: "es2022" },
  test: {
    include: ["src/**/*.test.{ts,tsx}", "scripts/**/*.test.mjs"],
    environment: "node",
  },
});
