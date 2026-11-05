import babel from "@rolldown/plugin-babel";
import tailwindcss from "@tailwindcss/vite";
import react, { reactCompilerPreset } from "@vitejs/plugin-react";
import { fileURLToPath } from "node:url";
import { defineConfig, type Plugin, type ViteBuilder } from "vite";
import { configDefaults } from "vitest/config";
import { BaseSequencer, type TestSpecification } from "vitest/node";
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

/** The compile-time constants every build of the app carries: the version About shows (src/settings/version.ts), from
 * tauri.conf.json, its one home. vite.review.config.ts takes them too. */
export const APP_DEFINE = { __CLAUSEO_VERSION__: JSON.stringify(tauriConf.version) };

/** The pages the release build makes, each its own entry with its own first load: the room, and the settings window.
 * The dev-only pages (gallery.html, parts.html, review.html) are not among them, so none of them ships. */
export const PAGES = {
  index: fileURLToPath(new URL("./index.html", import.meta.url)),
  settings: fileURLToPath(new URL("./settings.html", import.meta.url)),
};

/** One page's build: its HTML the only input, and everything its entry imports statically one chunk, its first
 * load, named after the page. */
function pageBuild(page: keyof typeof PAGES) {
  return {
    input: { [page]: PAGES[page] },
    output: { codeSplitting: { groups: [{ name: page, tags: ["$initial" as const] }] } },
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
        // the probe builds index.html alone (its input), the only page with this entry
        if (!html.includes(entry))
          throw new Error(`index.html has no ${entry} for the probe build to replace`);
        return html.replace(entry, '"/src/room/probe-main.tsx"');
      },
    },
  };
}

/** The chunks whose dynamic import is unwrapped: each imports only chunks already loaded when it loads, so there is
 * nothing to preload. The perf module and the entity table import nothing the first load lacks; the room's lazy parts
 * (the thread's list, the plan trigger, the working line's details, the composer's `/` menu) import only chunks
 * index.html preloads, and the list's change-note question only those and the list; U6's retried line only those, and
 * the title rules nothing; U7's queue strip only the first load; U16a's Find results only the first load and the small
 * jump-entry chunk they share with the list (isLazyShared); the tool line (its own chunk) only the first load, and its
 * popover only those and the line; UD1's first run only the first load (U2's form, which it shares with New matter, it
 * loads lazily itself, as Start a new matter is pressed). The Hindi words (ui/components' words-hi.ts, every words
 * file's .hi.ts) import only the first load's words registry: display.ts loads them before a Hindi display shows.
 * The dialogs that share the dialog primitives are
 * PRELOADED_CHUNKS, not here. vite-config.test.ts checks that, and that no chunk carries a preload list except the
 * importers of PRELOADED_CHUNKS. */
export const BARE_CHUNKS = [
  "perf",
  "character-entities",
  "list",
  "plan-trigger",
  "working-details",
  "retried-line",
  "conversation-title",
  "composer-menu",
  "queue-strip",
  "find-results",
  "tool-line-room",
  "tool-popover",
  "first-run",
  "words-hi",
];

/** The lazy chunks that keep Vite's preload list, though not bare: a lazy part behind a click whose chunk imports
 * shared chunks the first load lacks. Since the dialog primitives (dialog-start, DialogRoot, DialogDescription) left
 * the first load with New matter's dialog (room.tsx, lazy), these three import them: New matter, change-note-ask and
 * versions-chunk. Their preloads fetch the primitives in parallel, not in a waterfall after the click. vite-config.test.ts
 * checks that only the chunks importing one of these carry a preload list. */
export const PRELOADED_CHUNKS = ["new-matter", "change-note-ask", "versions-chunk"];

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
 * Unwraps the dynamic imports of BARE_CHUNKS from Vite's preload helper back to a plain `import()`, with their
 * preload lists: the perf module
 * (src/main.tsx; loaded only in the e2e and marks builds), the answer renderer's full entity table
 * (`import("character-entities")` in ui/patches/' decode-named-character-reference), and the room's lazy parts.
 * None has anything to preload that the first load doesn't already hold. Any other dynamic import keeps its preloads.
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

/** The test files that time the code: vitest's components-timed project, a group of its own. */
const TIMED = "src/**/*.timed.test.ts";
/**
 * The sequence.groupOrder of every other project: after components-timed (0) has finished. A new project needs it too:
 * without it, it is group 0 and runs beside the timed files.
 */
const AFTER_TIMED = 1;

/**
 * The test files that take longest, by their path under ui/: 7 to 35 s each on a 4-vCPU machine, against under 4 s for
 * nearly every other (hostile-stream-<k> and hostile-all are the hostile DOM tests, split so they run in parallel).
 * Not room/axe.test.tsx (about 18 s): beside these its first test took 3.5 to 5.1 s against its 5 s limit, and
 * failed 2 runs in 5; where vitest puts it, 1.5 to 1.9 s.
 */
export const SLOW_TESTS = [
  /\/components\/src\/markdown\/hostile-(?:all|stream-\d+)\.test\.tsx$/,
  /\/app\/src\/vite-config\.test\.ts$/,
  /\/components\/src\/markdown\/(?:split|repair|spec)\.test\.ts$/,
];

/**
 * Vitest's order with the slowest files (SLOW_TESTS) first. Without a results cache (CI starts from none) vitest
 * runs the app project's files before ui/components' and the larger files first, so the small files that stream the
 * hostile cases started last: the whole run waited about 50 s on one hostile file that began 60 s in.
 */
export class SlowFirst extends BaseSequencer {
  override async sort(files: TestSpecification[]): Promise<TestSpecification[]> {
    const sorted = await super.sort(files);
    const slow = (file: TestSpecification) =>
      SLOW_TESTS.some((pattern) => pattern.test(file.moduleId));
    return [...sorted.filter(slow), ...sorted.filter((file) => !slow(file))];
  }
}

export default defineConfig(({ mode }) => {
  const probe = mode === PROBE;
  return {
    plugins: [
      tailwindcss(),
      react(),
      babel({ presets: [reactCompilerPreset()] }),
      devCspHeader(),
      bareDynamicImports(),
      cspMeta(),
      ...(probe ? [probeEntry()] : []),
    ],
    // the probe is the room with the hooks for checks by hand (room/start.ts's HAND_CHECKS), which the probe runs
    define: {
      ...APP_DEFINE,
      ...(probe ? { "import.meta.env.VITE_CLAUSEO_HAND_CHECKS": JSON.stringify("1") } : {}),
    },
    server: {
      // Tauri's devUrl. The watcher stays out of the Rust crates and the multi-GB target folders, and out of
      // ui/app/components, the link to ui/components for oxlint: Vite already watches those files by their real path.
      port: 1420,
      strictPort: true,
      watch: { ignored: ["**/crates/**", "**/target/**", "**/app/components/**"] },
    },
    // WebView2 (Chromium), WKWebView (Safari 16+) and WebKitGTK all run ES2022. Fonts and every other asset load as
    // files under the CSP's 'self', never inlined as data: URLs.
    build: {
      target: "es2022",
      assetsInlineLimit: 0,
      // Everything the entry imports statically is one chunk, the first load, whichever lazy chunks also import it.
      // Left to itself, Rolldown puts each module that the entry and a lazy chunk both import into a chunk of its
      // own, so the first load is several files, and each new lazy chunk that imports shared modules changes the
      // split. On U8's unmerged branch (a new lazy chunk, Versions), initialJs rose by 2.8 KB brotli with the default
      // splitting.
      ...(probe ? { rolldownOptions: pageBuild("index"), outDir: "dist-probe" } : {}),
    },
    // Each page is a build of its own (an environment), so each first load is one chunk holding everything it needs,
    // and neither holds the other's code. One build of both pages would put what both import (React, the display
    // store) in a chunk of its own: the room's first load rose from 162.7 to 167.7 KB brotli that way, 1.5 KB of it
    // the imports between the chunks and the rest brotli compressing each alone. The cost is that copy on disk.
    // `vite build` builds every environment (`builder`), the room's first so it empties dist/; the probe is the room
    // alone (probeEntry).
    ...(probe
      ? {}
      : {
          environments: {
            client: { build: { rolldownOptions: pageBuild("index") } },
            settings: {
              consumer: "client" as const,
              build: { emptyOutDir: false, rolldownOptions: pageBuild("settings") },
            },
          },
          builder: {
            async buildApp(builder: ViteBuilder) {
              for (const name of ["client", "settings"])
                await builder.build(builder.environments[name]!);
            },
          },
        }),
    // The colouring worker (ui/components markdown/highlight/) as an ES module, so each Shiki grammar stays its own
    // chunk and loads when a block in that language first colours. The default `iife` cannot split, and inlines every
    // grammar into the worker: 862 KB.
    worker: { format: "es" },
    test: {
      sequence: { sequencer: SlowFirst },
      // One zone for every project and worker, whatever the shell's TZ, so local runs, CI and a machine in
      // Kolkata agree. Checked by src/testing-zone.test.ts (app) and components' src/testing/zone.test.ts, which
      // fail under a shell TZ=Asia/Kolkata without this line. clock.test.ts sets another zone inside one test, on purpose.
      env: { TZ: "UTC" },
      projects: [
        {
          extends: true,
          test: {
            name: "app",
            include: ["src/**/*.test.{ts,tsx}", "scripts/**/*.test.mjs"],
            environment: "node",
            // each test waits for its late imports (ui/components/src/lib/late.ts)
            setupFiles: ["../components/src/testing/setup.ts"],
            sequence: { groupOrder: AFTER_TIMED },
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
            exclude: [...configDefaults.exclude, TIMED],
            environment: "node",
            setupFiles: ["src/testing/setup.ts"],
            sequence: { groupOrder: AFTER_TIMED },
          },
        },
        {
          extends: true,
          test: {
            // ui/components' tests that time the code against itself (hostile-time-fixture.ts: each hostile case
            // against ordinary prose of its length, in thread CPU time). A group of their own, run before any other
            // test file starts, so they never share the machine with the DOM tests, and at most three at once, so
            // on a 4-vCPU CI runner each has a core. About 12 s on a 4-vCPU machine.
            name: "components-timed",
            root: "../components",
            include: [TIMED],
            environment: "node",
            setupFiles: ["src/testing/setup.ts"],
            sequence: { groupOrder: 0 },
            maxWorkers: 3,
            // CI only: a timed test that fails gets one more run, for a spike the per-round median did not absorb
            retry: process.env.CI ? 1 : 0,
          },
        },
      ],
    },
  };
});
