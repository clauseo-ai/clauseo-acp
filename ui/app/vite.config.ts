/// <reference types="vitest/config" />
import babel from "@rolldown/plugin-babel";
import tailwindcss from "@tailwindcss/vite";
import react, { reactCompilerPreset } from "@vitejs/plugin-react";
import { defineConfig, loadEnv, type Plugin } from "vite";
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

/**
 * The production `csp` as a page's meta tag can carry it: without `frame-ancestors`, which a meta tag may not hold
 * (browsers ignore it there and log a warning). Everything else unchanged.
 */
export function metaCsp(csp: string): string {
  return csp
    .split(";")
    .map((directive) => directive.trim())
    .filter((directive) => directive.length > 0 && directive.split(/\s+/)[0] !== "frame-ancestors")
    .join("; ");
}

/** A page's `<meta charset>`, which the HTML prescan must find in the page's first 1024 bytes. */
const CHARSET = /<meta charset="[^"]*"\s*\/?>/i;

/** `'` and `"` as an attribute value written in double quotes may carry them. */
const attr = (value: string) => value.replaceAll("&", "&amp;").replaceAll('"', "&quot;");

/**
 * The production CSP from tauri.conf.json (its one home), less `frame-ancestors`, as a built page's first element after
 * its `<meta charset>` (which stays first, inside the prescan's 1024 bytes) and so before any script, style or link, so
 * a page served without Tauri's header (`vite preview`, the WebKit probe) runs under the same policy and a runtime
 * <style>, a style attribute or an HTML string write shows up as a violation. Build only: the dev server sends its own
 * looser policy as a header (devCspHeader).
 */
export function cspMeta(): Plugin {
  const tag = `<meta http-equiv="Content-Security-Policy" content="${attr(metaCsp(tauriConf.app.security.csp))}">`;
  return {
    name: "clauseo-csp-meta",
    apply: "build",
    transformIndexHtml: {
      order: "post",
      handler(html) {
        const charset = CHARSET.exec(html);
        if (charset === null)
          throw new Error("the page has no <meta charset> for the CSP meta to follow");
        const at = charset.index + charset[0].length;
        return `${html.slice(0, at)}\n    ${tag}${html.slice(at)}`;
      },
    },
  };
}

/** The mode of `pnpm build:probe`. */
const PROBE = "probe";

/**
 * The probe build (`vite build --mode probe`, into dist-probe/): the room over FakeTransport, from
 * src/room/probe-main.tsx in place of src/main.tsx, for the WebKit probe (scripts/webkit-probe.swift) to load from
 * `vite preview --mode probe`. Not shipped.
 */
function probeEntry(): Plugin {
  return {
    name: "clauseo-probe-entry",
    transformIndexHtml: {
      order: "pre",
      handler(html) {
        const entry = '"/src/main.tsx"';
        if (!html.includes(entry))
          throw new Error(`index.html has no ${entry} for the probe build to replace`);
        return html.replace(entry, '"/src/room/probe-main.tsx"');
      },
    },
  };
}

/** The chunks whose dynamic import is unwrapped: each imports nothing the page would need to preload. */
const BARE_CHUNKS = ["perf", "character-entities"];

const HELPER = "__vitePreload(() => ";
const DEPS = ", __VITE_PRELOAD__)";

/**
 * `code` with every `__vitePreload(() => <body>, __VITE_PRELOAD__)` whose body starts with
 * `import("./<chunk>-` for a chunk in BARE_CHUNKS replaced by `<body>`. The body can run past the import
 * (`import(...).then(...)`), so its end is found by counting brackets, skipping string literals.
 */
export function unwrapPreloads(code: string, chunks: readonly string[] = BARE_CHUNKS): string {
  let out = "";
  let at = 0;
  for (;;) {
    const found = code.indexOf(HELPER, at);
    if (found < 0) return out + code.slice(at);
    const body = found + HELPER.length;
    if (!chunks.some((chunk) => code.startsWith(`import("./${chunk}-`, body))) {
      out += code.slice(at, body);
      at = body;
      continue;
    }
    const close = closingBracket(code, found + "__vitePreload".length);
    if (close < 0 || !code.slice(close - DEPS.length + 1, close + 1).endsWith(DEPS)) {
      throw new Error(`clauseo-bare-dynamic-imports: cannot unwrap the preload at ${found}`);
    }
    out += code.slice(at, found) + code.slice(body, close + 1 - DEPS.length);
    at = close + 1;
  }
}

/** The index of the `)` that closes the `(` at `open`, or -1. */
function closingBracket(code: string, open: number): number {
  let depth = 0;
  for (let i = open; i < code.length; i++) {
    const c = code[i]!;
    if (c === '"' || c === "'" || c === "`") {
      for (i++; i < code.length && code[i] !== c; i++) if (code[i] === "\\") i++;
    } else if (c === "(" || c === "[" || c === "{") depth++;
    else if (c === ")" || c === "]" || c === "}") {
      depth--;
      if (depth === 0) return c === ")" ? i : -1;
    }
  }
  return -1;
}

/**
 * Unwraps two dynamic imports from Vite's preload helper back to a plain `import()`, so the minifier drops the
 * helper, which would add about 1.8 KB to every release page's initial JS: the perf module (src/main.tsx; loaded only
 * in the e2e and marks builds), which imports only what the main chunk already holds, and the answer renderer's full
 * entity table (`import("character-entities")` in ui/patches/' decode-named-character-reference), a data module
 * with no imports. Neither has anything to preload. Any other dynamic import keeps its preloads.
 */
function bareDynamicImports(): Plugin {
  return {
    name: "clauseo-bare-dynamic-imports",
    apply: "build",
    enforce: "post",
    renderChunk(code) {
      const bare = unwrapPreloads(code);
      return bare === code ? null : bare;
    },
  };
}

export default defineConfig(({ mode }) => {
  const probe = mode === PROBE;
  // App.tsx's switch, as the build sees it: the room's build and the probe's carry the CSP as a meta tag too
  const room =
    probe || loadEnv(mode, process.cwd(), "VITE_CLAUSEO_")["VITE_CLAUSEO_WINDOW"] === "room";
  return {
    plugins: [
      tailwindcss(),
      react(),
      babel({ presets: [reactCompilerPreset()] }),
      devCspHeader(),
      bareDynamicImports(),
      ...(room ? [cspMeta()] : []),
      ...(probe ? [probeEntry()] : []),
    ],
    // the probe is the room with the hooks for checks by hand (room/start.ts's HAND_CHECKS), which the probe runs
    ...(probe
      ? {
          define: {
            "import.meta.env.VITE_CLAUSEO_WINDOW": JSON.stringify("room"),
            "import.meta.env.VITE_CLAUSEO_HAND_CHECKS": JSON.stringify("1"),
          },
        }
      : {}),
    server: {
      // Tauri's devUrl. The watcher stays out of the Rust crates and the multi-GB target folders, and out of
      // ui/app/components, the link to ui/components for oxlint: Vite already watches those files by their real path.
      port: 1420,
      strictPort: true,
      watch: { ignored: ["**/crates/**", "**/target/**", "**/app/components/**"] },
    },
    // WebView2 (Chromium), WKWebView (Safari 16+) and WebKitGTK all run ES2022. Fonts and every other asset load as
    // files under the CSP's 'self', never inlined as data: URLs.
    build: { target: "es2022", assetsInlineLimit: 0, ...(probe ? { outDir: "dist-probe" } : {}) },
    // The colouring worker (ui/components markdown/highlight/) as an ES module, so each Shiki grammar stays its own
    // chunk and loads when a block in that language first colours. The default `iife` cannot split, and inlines every
    // grammar into the worker: 862 KB.
    worker: { format: "es" },
    test: {
      projects: [
        {
          extends: true,
          test: {
            name: "app",
            include: ["src/**/*.test.{ts,tsx}", "scripts/**/*.test.mjs"],
            environment: "node",
            // each test waits for the room's late imports (src/room/late.ts)
            setupFiles: ["src/testing/setup.ts"],
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
  };
});
