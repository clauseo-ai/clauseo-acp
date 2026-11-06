import { brotliCompressSync } from "node:zlib";
import { createBuilder, createServer, resolveConfig, type Rolldown } from "vite";
import { afterEach, expect, test, vi } from "vitest";
import tauriConf from "../../../crates/desktop/tauri.conf.json" with { type: "json" };
import {
  BARE_CHUNKS,
  SETTINGS_BARE_CHUNKS,
  devCsp,
  metaCsp,
  PAGES,
  PRELOADED_CHUNKS,
  unwrapPreloads,
} from "../vite.config.ts";

/** A test that runs a whole Vite build: several seconds on a CI runner, past vitest's 5 s default. */
const BUILD = { timeout: 60_000 };

// The React Compiler skips a function it cannot compile instead of failing the build, so a broken
// Babel or compiler setup would only lose memoization. This loads the real vite.config.ts and
// transforms App.tsx the way the dev server does, so it fails if the compiler is not wired in.
// (`memo_cache_sentinel` in a bundle proves nothing: React's own runtime defines it.) The path is
// relative to ui/app, where `pnpm -C ui/app run test` runs vitest.
// The first test starts a Vite server and Babel cold: 6 s on a loaded runner, so 30 s of its own.
test("vite.config.ts builds App.tsx with the React Compiler", async () => {
  const server = await createServer({
    configFile: "vite.config.ts",
    server: { middlewareMode: true, watch: null },
    logLevel: "silent",
  });
  try {
    const result = await server.transformRequest("/src/App.tsx");
    // The dev server rewrites `react/compiler-runtime` to its prebundled `react_compiler-runtime`.
    expect(result?.code).toMatch(/react[/_]compiler-runtime/);
  } finally {
    await server.close();
  }
}, 30_000);

test("the dev CSP is the production CSP plus inline script and style and Vite's websocket", () => {
  const production = tauriConf.app.security.csp;
  const dev = devCsp(production, 1420);
  expect(dev).toContain("script-src 'self' 'unsafe-inline';");
  expect(dev).toContain("style-src 'self' 'unsafe-inline';");
  expect(dev).toContain("require-trusted-types-for 'script'");
  // Every production directive survives, in order, with only those additions.
  const strip = (policy: string) =>
    policy.replaceAll(" 'unsafe-inline'", "").replace(" ws://localhost:1420", "");
  expect(strip(dev)).toBe(production.replaceAll(/\s*;\s*/g, "; "));
});

test("the dev CSP allows Vite's websocket on its own port and no other", () => {
  const dev = devCsp(tauriConf.app.security.csp, 51234);
  const connect = dev.split("; ").find((directive) => directive.startsWith("connect-src"));
  expect(connect?.endsWith(" ws://localhost:51234")).toBe(true);
  expect(dev).not.toContain("localhost:*");
});

test("the dev CSP refuses a production CSP without a directive it adds to", () => {
  expect(() => devCsp("default-src 'self'; style-src 'self'; connect-src ipc:", 1420)).toThrow(
    /no script-src/,
  );
});

test("the build's only inputs are index.html and settings.html, each its own build, so the dev-only pages stay out of the shipped bundle", async () => {
  const config = await resolveConfig({ configFile: "vite.config.ts", logLevel: "silent" }, "build");
  const inputs = Object.fromEntries(
    Object.entries(config.environments).map(([name, env]) => [
      name,
      env.build.rolldownOptions.input,
    ]),
  );
  expect(inputs).toEqual({
    client: { index: PAGES.index },
    settings: { settings: PAGES.settings },
  });
  expect(PAGES.index.endsWith("/ui/app/index.html")).toBe(true);
  expect(PAGES.settings.endsWith("/ui/app/settings.html")).toBe(true);
  expect(config.build.assetsInlineLimit).toBe(0);
  expect(config.environments["settings"]?.build.assetsInlineLimit).toBe(0);
});

// Each page's first load is one chunk of its own, holding nothing of the other page: the room loads no settings code,
// and the settings window none of the room's frame, only the display store and the transport both use.
test(
  "each page's first load is one chunk, and neither page's holds the other's code",
  BUILD,
  async () => {
    const { pages } = await builtWith();
    expect(Object.keys(pages).sort()).toEqual(["index.html", "settings.html"]);
    const first = (page: string) => {
      const built = pages[page]!;
      const files = [...built.html.matchAll(/(?:src|href)="\/(assets\/[^"]+\.js)"/g)].map(
        (m) => m[1],
      );
      return files.map((file) => built.chunks.find((c) => c.fileName === file)!);
    };
    const room = first("index.html");
    const settings = first("settings.html");
    expect(room.map((c) => c.isEntry)).toEqual([true]);
    expect(settings.map((c) => c.isEntry)).toEqual([true]);
    expect(room[0]!.imports).toEqual([]);
    expect(settings[0]!.imports).toEqual([]);
    const modules = (chunk: Rolldown.OutputChunk) =>
      chunk.moduleIds.map((id) => id.split("/src/")[1] ?? id);
    expect(modules(room[0]!).filter((id) => id.startsWith("settings/"))).toEqual([]);
    expect(modules(room[0]!)).toContain("room/room.tsx");
    expect(modules(settings[0]!)).toContain("settings/main.tsx");
    expect(modules(settings[0]!)).toContain("room/display-store.ts");
    for (const frame of ["room/room.tsx", "room/rail.tsx", "main.tsx", "App.tsx"])
      expect(modules(settings[0]!)).not.toContain(frame);
    expect(settings[0]!.code).not.toContain("room-window");
    // Connectors' body (U15) and Plans' (UD2) are chunks of their own, imported bare: no preload helper, so their CSS
    // is the entry's
    const lazy = pages["settings.html"]!.chunks.filter((c) =>
      SETTINGS_BARE_CHUNKS.some((name) => c.fileName.startsWith(`assets/${name}-`)),
    );
    expect(lazy.map((c) => c.isDynamicEntry)).toEqual(SETTINGS_BARE_CHUNKS.map(() => true));
    for (const name of SETTINGS_BARE_CHUNKS) {
      const chunk = lazy.find((c) => c.fileName.startsWith(`assets/${name}-`));
      expect(settings[0]!.dynamicImports).toContain(chunk!.fileName);
      expect(settings[0]!.code).not.toContain(`__vitePreload(() => import("./${name}-`);
      expect(settings[0]!.code).toMatch(new RegExp(`import\\(.\\./${name}-`));
      // each imports the first load, and at most a part the two share (TextField), which only they import
      for (const file of chunk!.imports) {
        if (file === settings[0]!.fileName) continue;
        const importers = pages["settings.html"]!.chunks.filter((c) => c.imports.includes(file));
        expect(
          importers.every((c) => lazy.includes(c)),
          file,
        ).toBe(true);
      }
    }
    expect(modules(settings[0]!)).not.toContain("settings/sections/connectors.tsx");
    for (const lazyPart of ["settings/sections/plans.tsx", "store/setup.ts"])
      expect(modules(settings[0]!)).not.toContain(lazyPart);
  },
);

// The perf module (src/perf/) loads only in the e2e and marks builds; a release page must not pay for it. This builds
// the app in memory and checks the perf code sits in its own chunk, imported with a bare import() from the entry
// chunk, and the entity table in its own, imported with a bare import() from the chunk that decodes entities: neither
// goes through Vite's preload helper, whose call would wrap it in an arrow (vite.config.ts's bareDynamicImports).
test(
  "the release build keeps the perf module, the entity table and the room's lazy parts in chunks of their own, each imported without a preload",
  BUILD,
  async () => {
    // as `vite build` runs it (builtWith): under vitest's NODE_ENV=test the build would be a dev one, with the hooks
    const { chunks } = (await builtWith()).pages["index.html"]!;
    const entry = chunks.find((c) => c.isEntry);
    const perf = chunks.find((c) => c.fileName.startsWith("assets/perf-"));
    expect(entry).toBeDefined();
    expect(perf).toBeDefined();
    expect(perf!.isDynamicEntry).toBe(true);
    expect(entry!.code).toMatch(/[^>]import\(`\.\/perf-[\w-]+\.js`\)/);
    const entities = chunks.find((c) => c.fileName.startsWith("assets/character-entities-"));
    expect(entities).toBeDefined();
    expect(entities!.isDynamicEntry).toBe(true);
    const decoders = chunks.filter((c) => /import\(`\.\/character-entities-/.test(c.code));
    expect(decoders.length).toBe(1);
    expect(decoders[0]!.code).toMatch(/[^>]import\(`\.\/character-entities-[\w-]+\.js`\)\.then\(/);
    expect(entry!.code).not.toContain("startStream");
    // U17: a tool result's parser, block and blob read ride in the result chunk alone, loaded on the first press
    const parser = "/room/thread/tool-result/parse.ts";
    const holding = chunks.filter((c) => c.moduleIds.some((id) => id.endsWith(parser)));
    expect(holding.map((c) => c.fileName.split("-")[0])).toEqual(["assets/result"]);
    // no preload list anywhere but in the importers of a PRELOADED_CHUNKS chunk: every other dynamic import in the build
    // is a bare one. (The helper itself stays in the entry, exported, since the chunks are linked before renderChunk
    // unwraps its calls.)
    const preloaded = PRELOADED_CHUNKS.map((name) =>
      chunks.find((c) => c.fileName.startsWith(`assets/${name}-`)),
    );
    expect(preloaded.map((c) => c?.isDynamicEntry)).toEqual(PRELOADED_CHUNKS.map(() => true));
    const importsPreloaded = (chunk: (typeof chunks)[number]) =>
      chunk.dynamicImports.some((file) => preloaded.some((c) => c?.fileName === file));
    for (const chunk of chunks)
      if (!importsPreloaded(chunk))
        expect(chunk.code, chunk.fileName).not.toContain("__vite__mapDeps");
    // and unwrapping lost no preload: each bare chunk imports only what is loaded by the time it loads, the first
    // load and the chunk that imports it with what that one imports
    const byName = new Map(chunks.map((c) => [c.fileName, c]));
    const closure = (name: string, into = new Set<string>()) => {
      if (into.has(name)) return into;
      into.add(name);
      for (const next of byName.get(name)?.imports ?? []) closure(next, into);
      return into;
    };
    const first = closure(entry!.fileName);
    // the first load is one chunk (vite.config.ts's codeSplitting): the entry statically imports no other file
    expect([...first]).toEqual([entry!.fileName]);
    const bare = chunks.filter((c) =>
      BARE_CHUNKS.some((name) => c.fileName.startsWith(`assets/${name}-`)),
    );
    expect(bare.length).toBeGreaterThanOrEqual(BARE_CHUNKS.length);
    // the one exception (isLazyShared): a small chunk that only bare chunks import, and that the first load lacks.
    // Nothing the entry imports reaches it, so it loads with the bare chunk that imports it
    for (const chunk of bare) {
      expect(chunk.isDynamicEntry, chunk.fileName).toBe(true);
      const importers = chunks.filter((c) => c.dynamicImports.includes(chunk.fileName));
      expect(importers.length, chunk.fileName).toBeGreaterThan(0);
      for (const importer of importers) {
        const loaded = new Set([...first, ...closure(importer.fileName)]);
        for (const dep of chunk.imports)
          expect(
            loaded.has(dep) || isLazyShared(dep, chunks, first, bare),
            `${chunk.fileName}: ${dep}`,
          ).toBe(true);
      }
    }
  },
);

/** A chunk as the build reports it, with only the fields the rule reads. */
type Chunk = Pick<Rolldown.OutputChunk, "fileName" | "code" | "imports">;

/**
 * The exception to the bare-chunk rule: `dep` may be missing from the first load when it is a chunk outside `first`
 * (the first load's file names), under 1000 B brotli, that only bare chunks import (at least one does), and whose own
 * imports are all in the first load (so the exception adds no chunk that loads without a preload). Such a chunk is
 * reached by no static import of the entry, so it loads with the bare chunk that imports it, and no sooner.
 */
export function isLazyShared(
  dep: string,
  chunks: readonly Chunk[],
  first: ReadonlySet<string>,
  bare: readonly Chunk[],
): boolean {
  const target = chunks.find((c) => c.fileName === dep);
  if (target === undefined || first.has(dep)) return false;
  if (!target.imports.every((d) => first.has(d))) return false;
  const bareNames = new Set(bare.map((c) => c.fileName));
  const importers = chunks.filter((c) => c.imports.includes(dep));
  return (
    importers.length > 0 &&
    importers.every((c) => bareNames.has(c.fileName)) &&
    brotliCompressSync(target.code).length < 1000
  );
}

/** `n` characters of a seeded LCG, 7 bits each, so brotli cannot shrink them much. Shorter output is a prefix. */
function noise(n: number): string {
  let state = 12345;
  let out = "";
  for (let i = 0; i < n; i++) {
    state = (Math.imul(state, 1664525) + 1013904223) >>> 0;
    out += String.fromCharCode(state >>> 25);
  }
  return out;
}

const brotliSize = (code: string) => brotliCompressSync(code).length;

/** The smallest length whose noise brotli-compresses to 1000 B or more, by bisection (brotli grows with length here). */
function firstLengthAtLimit(): number {
  let below = 0;
  let atOrAbove = 8000;
  expect(brotliSize(noise(atOrAbove))).toBeGreaterThanOrEqual(1000);
  while (atOrAbove - below > 1) {
    const mid = (below + atOrAbove) >> 1;
    if (brotliSize(noise(mid)) < 1000) below = mid;
    else atOrAbove = mid;
  }
  return atOrAbove;
}

// Synthetic chunks. The first load is `index`; `list` and `plan` are bare; `shared` is the chunk under test.
const synth = (fileName: string, imports: string[] = [], code = "export {};"): Chunk => ({
  fileName,
  imports,
  code,
});
const FIRST = new Set(["assets/index.js"]);

test("isLazyShared is true for a small chunk that only bare chunks import, outside the first load", () => {
  const list = synth("assets/list.js", ["assets/shared.js"]);
  const plan = synth("assets/plan.js", ["assets/shared.js"]);
  const chunks = [synth("assets/index.js"), list, plan, synth("assets/shared.js")];
  expect(isLazyShared("assets/shared.js", chunks, FIRST, [list, plan])).toBe(true);
});

test("isLazyShared is false when a chunk that is not bare also imports it", () => {
  const list = synth("assets/list.js", ["assets/shared.js"]);
  const markdown = synth("assets/markdown.js", ["assets/shared.js"]);
  const chunks = [synth("assets/index.js"), list, markdown, synth("assets/shared.js")];
  expect(isLazyShared("assets/shared.js", chunks, FIRST, [list])).toBe(false);
});

test("isLazyShared is true just under 1000 B brotli and false from 1000 B up", () => {
  const limit = firstLengthAtLimit();
  const list = synth("assets/list.js", ["assets/shared.js"]);
  const under = synth("assets/shared.js", [], noise(limit - 1));
  const over = synth("assets/shared.js", [], noise(limit));
  // the edge: the first length at 1000 B or more (the length before it is under 1000 B)
  expect(brotliSize(under.code)).toBeLessThan(1000);
  expect(brotliSize(over.code)).toBeGreaterThanOrEqual(1000);
  expect(
    isLazyShared("assets/shared.js", [synth("assets/index.js"), list, under], FIRST, [list]),
  ).toBe(true);
  expect(
    isLazyShared("assets/shared.js", [synth("assets/index.js"), list, over], FIRST, [list]),
  ).toBe(false);
});

test("isLazyShared is false for a chunk the first load holds", () => {
  const list = synth("assets/list.js", ["assets/shared.js"]);
  const chunks = [synth("assets/index.js"), list, synth("assets/shared.js")];
  const first = new Set([...FIRST, "assets/shared.js"]);
  expect(isLazyShared("assets/shared.js", chunks, first, [list])).toBe(false);
});

test("isLazyShared is false when the shared chunk imports a chunk outside the first load", () => {
  const list = synth("assets/list.js", ["assets/shared.js"]);
  const shared = synth("assets/shared.js", ["assets/big.js"]);
  const chunks = [synth("assets/index.js"), list, shared, synth("assets/big.js")];
  expect(isLazyShared("assets/shared.js", chunks, FIRST, [list])).toBe(false);
  // the same chunk with its import in the first load is the exception
  const inFirst = new Set([...FIRST, "assets/big.js"]);
  expect(isLazyShared("assets/shared.js", chunks, inFirst, [list])).toBe(true);
});

test("isLazyShared is false for a name no chunk has, and for a chunk nothing imports", () => {
  const list = synth("assets/list.js");
  const chunks = [synth("assets/index.js"), list];
  expect(isLazyShared("assets/missing.js", chunks, FIRST, [list])).toBe(false);
  expect(isLazyShared("assets/list.js", chunks, FIRST, [list])).toBe(false);
});

/** One environment's build: its chunks, its other files and its page. */
type Built = {
  chunks: Rolldown.OutputChunk[];
  files: (Rolldown.OutputChunk | Rolldown.OutputAsset)[];
  html: string;
};

/** A build in memory under `mode`, every environment as `vite build` builds it: each page's build by its HTML file's
 * name, and every chunk and file of them all, with index.html's text. */
async function builtWith(mode?: string) {
  // as `vite build` runs it: vitest's NODE_ENV=test would make the build a development one (import.meta.env.DEV)
  vi.stubEnv("NODE_ENV", "production");
  const builder = await createBuilder({
    configFile: "vite.config.ts",
    logLevel: "silent",
    ...(mode === undefined ? {} : { mode }),
    build: { write: false },
  });
  const pages: Record<string, Built> = {};
  for (const environment of Object.values(builder.environments)) {
    const out = (await builder.build(environment)) as
      | Rolldown.RolldownOutput
      | Rolldown.RolldownOutput[];
    const files = (Array.isArray(out) ? out : [out]).flatMap((o) => o.output);
    const chunks = files.filter((c): c is Rolldown.OutputChunk => c.type === "chunk");
    const htmls = files.filter(
      (f): f is Rolldown.OutputAsset => f.type === "asset" && f.fileName.endsWith(".html"),
    );
    expect(htmls.length, environment.name).toBe(1);
    pages[htmls[0]!.fileName] = { chunks, files, html: String(htmls[0]!.source) };
  }
  const all = Object.values(pages);
  return {
    pages,
    chunks: all.flatMap((p) => p.chunks),
    files: all.flatMap((p) => p.files),
    html: pages["index.html"]?.html ?? "",
  };
}

// the page's charset first, inside the HTML prescan's 1024 bytes, then the CSP before any script, style or link
const META =
  /^<!doctype html>\s*<html[^>]*>\s*<head>\s*<meta charset="UTF-8"\s*\/?>\s*<meta http-equiv="Content-Security-Policy" content="([^"]*)">/i;
const unescape = (html: string) => html.replaceAll("&#39;", "'").replaceAll("&quot;", '"');

afterEach(() => {
  vi.unstubAllEnvs();
});

test("the meta CSP is the production CSP less frame-ancestors, which a meta tag may not carry", () => {
  const production = tauriConf.app.security.csp;
  expect(production).toContain("frame-ancestors 'none'");
  const meta = metaCsp(production);
  expect(meta).not.toContain("frame-ancestors");
  expect(meta.startsWith("default-src 'self'; script-src 'self'; style-src 'self'; ")).toBe(true);
  expect(meta).toContain(
    "; base-uri 'none'; form-action 'none'; require-trusted-types-for 'script'; ",
  );
  expect(
    meta.endsWith("; require-trusted-types-for 'script'; trusted-types clauseo-sanitize"),
  ).toBe(true);
  // one directive fewer than the production policy, which has 17
  expect(production.split(";").length).toBe(17);
  expect(meta.split("; ").length).toBe(16);
});

/** What only the copy review page holds: none of it may reach the release build. */
const REVIEW_ONLY = ["U1-01", "copy-approvals"];

/** The build's hooks for checks by hand and axe-core's chunk, where it has them. */
function handCheckChunks(chunks: Rolldown.OutputChunk[]) {
  return {
    hooks: chunks.filter(
      (c) => c.fileName.startsWith("assets/dev-hooks-") || c.code.includes("__clauseoAxe"),
    ),
    axe: chunks.filter(
      (c) =>
        c.fileName.startsWith("assets/axe") ||
        c.code.includes("axe.run") ||
        c.moduleIds.some((id) => id.includes("/axe-core/")),
    ),
  };
}

// The release build, without VITE_CLAUSEO_HAND_CHECKS: the room and its stylesheet, the policy as its head's first
// element after the charset, and neither the copy review page, the hooks nor axe-core anywhere in it.
test(
  "the release build is the room under the meta CSP, with no review page, hooks or axe-core chunk",
  BUILD,
  async () => {
    const { html, chunks, files, pages } = await builtWith();
    expect(unescape(META.exec(html)?.[1] ?? "")).toBe(metaCsp(tauriConf.app.security.csp));
    // the settings window's page under the same policy; no other page is built (gallery, parts, review)
    expect(unescape(META.exec(pages["settings.html"]?.html ?? "")?.[1] ?? "")).toBe(
      metaCsp(tauriConf.app.security.csp),
    );
    expect(
      files
        .filter((f) => f.fileName.endsWith(".html"))
        .map((f) => f.fileName)
        .sort(),
    ).toEqual(["index.html", "settings.html"]);
    expect(chunks.some((c) => c.moduleIds.some((id) => id.includes("/src/room/")))).toBe(true);
    expect(chunks.some((c) => c.code.includes("room-window"))).toBe(true);
    expect(files.some((f) => f.fileName.endsWith(".css"))).toBe(true);
    for (const chunk of chunks) {
      for (const word of REVIEW_ONLY)
        expect(chunk.code, `${chunk.fileName}: ${word}`).not.toContain(word);
      const strays = chunk.moduleIds.filter((id) => id.includes("/src/review/"));
      expect(strays, chunk.fileName).toEqual([]);
    }
    expect(handCheckChunks(chunks)).toEqual({ hooks: [], axe: [] });
  },
);

// With VITE_CLAUSEO_HAND_CHECKS=1, the e2e build: the hooks (room/dev-hooks.ts) and axe-core in chunks of their
// own that the page doesn't load at start.
test(
  "the build with VITE_CLAUSEO_HAND_CHECKS=1 keeps its hooks and axe-core in chunks of their own",
  BUILD,
  async () => {
    vi.stubEnv("VITE_CLAUSEO_HAND_CHECKS", "1");
    const { html, chunks } = await builtWith();
    const { hooks, axe } = handCheckChunks(chunks);
    expect(hooks.map((c) => c.isDynamicEntry)).toEqual([true]);
    expect(axe.map((c) => c.isDynamicEntry)).toEqual([true]);
    const initial = new Set(
      [...html.matchAll(/(?:src|href)="\/(assets\/[^"]+\.js)"/g)].map((m) => m[1]),
    );
    expect(initial.has(hooks[0]?.fileName ?? "")).toBe(false);
    expect(initial.has(axe[0]?.fileName ?? "")).toBe(false);
  },
);

test(
  "the probe build mounts the room over FakeTransport from room/probe-main.tsx, under the meta CSP, with the hooks",
  BUILD,
  async () => {
    const config = await resolveConfig(
      { configFile: "vite.config.ts", logLevel: "silent", mode: "probe" },
      "build",
    );
    expect(config.build.outDir).toBe("dist-probe");
    const { html, chunks, pages } = await builtWith("probe");
    expect(Object.keys(pages)).toEqual(["index.html"]);
    expect(unescape(META.exec(html)?.[1] ?? "")).toBe(metaCsp(tauriConf.app.security.csp));
    const entry = chunks.find((c) => c.isEntry);
    const modules = entry?.moduleIds ?? [];
    expect(modules.some((id) => id.endsWith("/src/room/probe-main.tsx"))).toBe(true);
    expect(modules.some((id) => id.endsWith("/src/main.tsx"))).toBe(false);
    expect(handCheckChunks(chunks).axe.length).toBe(1);
  },
);

test("unwrapPreloads unwraps only the listed chunks, bodies with brackets and strings included", () => {
  const wrapped = (body: string) => `__vitePreload(() => ${body}, __VITE_PRELOAD__)`;
  const entities =
    'import("./character-entities-x1.js").then(function (m) { t = m[")"]; }, function (e) { throw e; })';
  const perf = 'import("./perf-x2.js")';
  const other = 'import("./html-x3.js")';
  expect(
    unwrapPreloads(`a = ${wrapped(entities)}; b = ${wrapped(perf)}; c = ${wrapped(other)};`),
  ).toBe(`a = ${entities}; b = ${perf}; c = ${wrapped(other)};`);
  expect(() => unwrapPreloads('__vitePreload(() => import("./perf-x.js")')).toThrow(
    /cannot unwrap/,
  );
});
