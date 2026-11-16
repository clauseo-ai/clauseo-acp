import { readdirSync, readFileSync } from "node:fs";
import { expect, test } from "vitest";
import tauriConf from "../../../crates/desktop/tauri.conf.json" with { type: "json" };
import { PAGES } from "../vite.config.ts";

// The app runs under the production CSP in tauri.conf.json. ui/components' parts must work under it as they are:
// no inline style or script, no runtime <style>, nothing loaded from the web, fonts as files from the app's own
// origin. gallery/gallery.test.tsx renders the controls and checks that no <style> element appears; this checks the
// policy and the sources.
const directives = new Map(
  tauriConf.app.security.csp
    .split(";")
    .map((d) => d.trim().split(/\s+/))
    .filter((d) => d.length > 1)
    .map(([name, ...sources]) => [name, sources] as const),
);

// The host's own schemes reach the webview as http(s)://<scheme>.localhost on Windows and Linux; those stay local.
const web = (source: string) =>
  /^\*$|^https?:/.test(source) && !/^https?:\/\/[\w-]+\.localhost$/.test(source);

test("the production CSP allows styles, scripts and fonts only from the app itself, and nothing from the web", () => {
  for (const name of ["style-src", "script-src", "font-src"]) {
    expect(directives.get(name), name).toEqual(["'self'"]);
  }
  for (const [name, sources] of directives) {
    expect(sources.filter(web), name).toEqual([]);
  }
});

/** What a part could do that the CSP refuses: write HTML as a string, or add a <style> element at runtime. */
const REFUSED: [string, RegExp][] = [
  ["dangerouslySetInnerHTML", /dangerouslySetInnerHTML/],
  ["innerHTML or outerHTML", /\.(?:inner|outer)HTML\s*=/],
  ["insertAdjacentHTML", /insertAdjacentHTML/],
  ["document.write", /document\.write/],
  ["a <style> element", /createElement\(\s*["']style["']/],
  ["a stylesheet built at runtime", /new CSSStyleSheet|adoptedStyleSheets/],
];

test("ui/components writes no HTML strings and adds no <style> at runtime", () => {
  const src = new URL("../../components/src/", import.meta.url);
  const found: string[] = [];
  let scanned = 0;
  for (const file of readdirSync(src, { recursive: true })) {
    if (!/\.tsx?$/.test(file) || /\.test\.tsx?$/.test(file)) continue;
    scanned++;
    const text = readFileSync(new URL(file, src), "utf8");
    for (const [what, rule] of REFUSED) if (rule.test(text)) found.push(`${file}: ${what}`);
  }
  expect(scanned).toBeGreaterThan(30);
  expect(found).toEqual([]);
});

// The pages the app ships (vite.config.ts's PAGES): the room's and the settings window's. index.html's one <style>, the
// first paint before the app's CSS, is the exception below.
test.each(["index.html", "settings.html"])(
  "the app's page %s has no inline script or style attribute, and no <style> but the first paint's",
  (page) => {
    const html = readFileSync(new URL(`../${page}`, import.meta.url), "utf8");
    expect(html).not.toMatch(/style=/);
    expect([...html.matchAll(/<style/g)].length).toBe(page === "index.html" ? 1 : 0);
    const scripts = [...html.matchAll(/<script([^>]*)>([\s\S]*?)<\/script>/g)];
    expect(scripts.length).toBe(1);
    for (const m of scripts) {
      expect(m[1]).toMatch(/\bsrc=/);
      expect(m[2]?.trim()).toBe("");
    }
  },
);

test("the pages checked for inline script or style are every page the build makes", () => {
  expect(Object.values(PAGES).map((path) => path.split("/").pop())).toEqual([
    "index.html",
    "settings.html",
  ]);
});

// index.html's first paint: one inline <style>. Tauri's header allows it by the nonce Tauri adds to the element and to
// the header's style-src when it serves the page (tauri-utils html.rs, inject_nonce_token; tauri manager/mod.rs,
// replace_csp_nonce), unless the config turns asset CSP modification off. The page's meta CSP allows it by its hash
// (vite.config.ts, cspMeta; vite-config.test.ts pins it).
test("index.html's inline <style> is allowed by Tauri's nonce, which the config leaves on", () => {
  const security: Record<string, unknown> = tauriConf.app.security;
  expect(security["dangerousDisableAssetCspModification"]).toBeUndefined();
});

// The first paint's mark is the lockup's comma (ui/components lockup.tsx), its path as drawn there, and nothing else.
test("index.html's first mark is the lockup's comma, no text, and breathes only in opacity", () => {
  const html = readFileSync(new URL("../index.html", import.meta.url), "utf8");
  const lockup = readFileSync(
    new URL("../../components/src/clauseo/lockup.tsx", import.meta.url),
    "utf8",
  );
  const comma = /const COMMA =\s*"([^"]+)"/.exec(lockup)?.[1];
  const paths = [...html.matchAll(/<path\s+d="([^"]+)"/g)].map((m) => m[1]);
  expect(comma).toBeDefined();
  expect(paths).toEqual([comma]);
  expect(html).not.toMatch(/<text/);
  // one animation, the mark's: opacity 0.55 to 1 and back, none under reduced motion; the ground never moves
  expect([...html.matchAll(/animation:\s*([^;]+);/g)].map((m) => m[1])).toEqual([
    "first-breath 2.4s ease-in-out infinite alternate",
    "none",
  ]);
  const breath = /@keyframes first-breath \{([\s\S]*?)\n {6}\}/.exec(html)?.[1] ?? "";
  expect(breath.replace(/\s+/g, "")).toBe("from{opacity:0.55;}to{opacity:1;}");
  expect(html).toMatch(
    /@media \(prefers-reduced-motion: reduce\) \{\s*#first-mark svg \{\s*animation: none;/,
  );
  const root = /<div id="root">([\s\S]*?)<\/div>\s*<script/.exec(html)?.[1] ?? "";
  expect(root).toContain('id="first-mark"');
});

// Its colours are the tokens' hex comments: --ground behind, --ground-press for the comma, day then night.
test("index.html's first paint takes --ground and --ground-press from tokens-colour.css", () => {
  const html = readFileSync(new URL("../index.html", import.meta.url), "utf8").toLowerCase();
  const tokens = readFileSync(
    new URL("../../components/src/styles/tokens-colour.css", import.meta.url),
    "utf8",
  );
  const hexes = (name: string) =>
    [...tokens.matchAll(new RegExp(`^\\s*--${name}:[^;]*;\\s*/\\* (#[0-9A-Fa-f]{6})`, "gm"))].map(
      (m) => m[1]?.toLowerCase(),
    );
  const [dayGround, nightGround] = hexes("ground");
  const [dayPress, nightPress] = hexes("ground-press");
  for (const hex of [dayGround, nightGround, dayPress, nightPress]) {
    expect(hex).toBeDefined();
    expect(html).toContain(`${hex};`);
  }
  const sorted = (hexes: (string | undefined)[]) =>
    hexes.map(String).sort((x, y) => x.localeCompare(y));
  expect(sorted(html.match(/#[0-9a-f]{6}/g) ?? [])).toEqual(
    sorted([dayGround, nightGround, dayPress, nightPress, nightPress]),
  );
});
