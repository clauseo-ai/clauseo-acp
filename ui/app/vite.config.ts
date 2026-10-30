/// <reference types="vitest/config" />
import babel from "@rolldown/plugin-babel";
import tailwindcss from "@tailwindcss/vite";
import react, { reactCompilerPreset } from "@vitejs/plugin-react";
import { defineConfig, type Plugin } from "vite";
import tauriConf from "../../crates/desktop/tauri.conf.json" with { type: "json" };

/** The directives the dev policy loosens, and what each gets, for a dev server on `port`. */
function devAdditions(port: number): Record<string, string> {
  return {
    // React Refresh's preamble is an inline script.
    "script-src": "'unsafe-inline'",
    // Vite injects CSS as <style> elements in dev.
    "style-src": "'unsafe-inline'",
    // Vite's HMR websocket, on exactly the port this dev server listens on: 1420 for Tauri's
    // devUrl, or the free port `cargo xtask buildtime web` picks.
    "connect-src": `ws://localhost:${port}`,
  };
}

/**
 * The dev CSP for a dev server on `port`: the production `csp` from tauri.conf.json plus
 * `devAdditions`, everything else (Trusted Types included) unchanged. Built here because in dev
 * the page comes from Vite, not Tauri's protocol, so Tauri sets no CSP header and Vite is the only
 * one that can.
 */
export function devCsp(csp: string, port: number): string {
  const additions = devAdditions(port);
  const directives = csp
    .split(";")
    .map((directive) => directive.trim())
    .filter((directive) => directive.length > 0);
  for (const name of Object.keys(additions)) {
    if (!directives.some((directive) => directive.split(/\s+/)[0] === name)) {
      throw new Error(`tauri.conf.json app.security.csp has no ${name}; the dev CSP adds to it`);
    }
  }
  return directives
    .map((directive) => {
      const addition = additions[directive.split(/\s+/)[0] ?? ""];
      return addition === undefined ? directive : `${directive} ${addition}`;
    })
    .join("; ");
}

/**
 * Sends the dev CSP on every dev server response, with the port the server was started on
 * (`--port` on the command line overrides the 1420 below; `strictPort` means it is the port it
 * listens on).
 */
function devCspHeader(): Plugin {
  return {
    name: "clauseo-dev-csp",
    apply: "serve",
    configureServer(server) {
      const policy = devCsp(tauriConf.app.security.csp, server.config.server.port);
      server.middlewares.use((_request, response, next) => {
        response.setHeader("Content-Security-Policy", policy);
        next();
      });
    },
  };
}

export default defineConfig({
  plugins: [tailwindcss(), react(), babel({ presets: [reactCompilerPreset()] }), devCspHeader()],
  server: {
    // Tauri's devUrl. The watcher stays out of the Rust crates and the multi-GB target folders, and out of
    // ui/app/components, the link to ui/components for oxlint: Vite already watches those files by their real path.
    port: 1420,
    strictPort: true,
    watch: { ignored: ["**/crates/**", "**/target/**", "**/app/components/**"] },
  },
  // WebView2 (Chromium), WKWebView (Safari 16+) and WebKitGTK all run ES2022. Fonts and every other asset load as
  // files under the CSP's 'self', never inlined as data: URLs.
  build: { target: "es2022", assetsInlineLimit: 0 },
  test: {
    projects: [
      {
        extends: true,
        test: {
          name: "app",
          include: ["src/**/*.test.{ts,tsx}", "scripts/**/*.test.mjs"],
          environment: "node",
        },
      },
      {
        extends: true,
        test: {
          name: "components",
          // ui/components' tests that ask for "// @vitest-environment happy-dom" get it from ui/app's devDependencies:
          // vitest runs from here, so the environment resolves here.
          root: "../components",
          include: ["src/**/*.test.{ts,tsx}"],
          environment: "node",
        },
      },
    ],
  },
});
