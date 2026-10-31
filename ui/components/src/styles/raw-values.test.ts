import { readdirSync, readFileSync } from "node:fs";
import { expect, test } from "vitest";

// No raw values in the controls, icons and Clauseo's parts: every colour, size and shadow comes from a token class.
// No published lint rule checks Tailwind classes against a token set, so this scan stands in for one.
const roots = ["../ui/", "../icons/", "../clauseo/"].map((p) => new URL(p, import.meta.url));

/** Rules on the code itself. */
const CODE: [string, RegExp][] = [
  ["a colour function", /\b(?:rgba?|hsla?|oklch|oklab|lab|lch|color-mix|hwb)\(/],
  ["a style attribute", /\bstyle=\{/],
];
/** Rules on each word inside a string, where class names live. */
const WORD: [string, RegExp][] = [
  ["a hex colour", /#[0-9a-fA-F]{3,8}$/],
  ["an arbitrary value, property or variant", /^[!\w:@&-]*\[[^\]]*\]/],
  ["a raw length", /^[\w:-]*-\d+(?:\.\d+)?(?:px|rem|em|vw|vh)$/],
];
const STRINGS = /"([^"\\]*)"|'([^'\\]*)'|`([^`\\]*)`/g;

/** Every problem in one file's text, as "line: what: text". */
export function scan(text: string): string[] {
  const found: string[] = [];
  text.split("\n").forEach((line, i) => {
    const code = line.replace(/\/\/.*$|\/\*.*?\*\/|^\s*\*.*$/g, "");
    for (const [what, rule] of CODE)
      if (rule.test(code)) found.push(`${i + 1}: ${what}: ${line.trim()}`);
    for (const m of code.matchAll(STRINGS)) {
      for (const word of (m[1] ?? m[2] ?? m[3] ?? "").split(/\s+/)) {
        for (const [what, rule] of WORD)
          if (rule.test(word)) found.push(`${i + 1}: ${what} "${word}"`);
      }
    }
  });
  return found;
}

test("no raw colours, arbitrary values or style attributes in the controls, icons and parts", () => {
  const found: string[] = [];
  let scanned = 0;
  for (const root of roots) {
    for (const file of readdirSync(root, { recursive: true })) {
      if (!/\.(tsx?|css)$/.test(file) || /\.test\.tsx?$/.test(file)) continue;
      scanned++;
      for (const f of scan(readFileSync(new URL(file, root), "utf8"))) found.push(`${file}:${f}`);
    }
  }
  expect(scanned).toBeGreaterThan(10);
  expect(found).toEqual([]);
});

/* ---------- the stylesheets ---------- */

// Every length, colour and duration in a part's stylesheet comes from a token, through var(). A one-off with a
// reason is a named token in tokens-parts.css, in the block for Clauseo's parts; a hairline too. Zero, percentages,
// angles and plain numbers (a scale, an opacity, a weight) aren't values a token holds. The base and primitives
// stylesheets set the page's own geometry and are not held to it. The stylesheets of Clauseo's parts hold no raw
// values.
const cssRoot = new URL("../clauseo/", import.meta.url);

const UNITS = "px|rem|em|vw|vh|dvh|svh|lvh|vmin|vmax|ch|ex|lh|cqw|cqh|cqi|cqb|cm|mm|in|pt|pc";
const CSS: [string, RegExp][] = [
  ["a raw length", new RegExp(`(?<![\\w.#-])-?(?:\\d+\\.?\\d*|\\.\\d+)(?:${UNITS})\\b`, "g")],
  ["a raw duration", /(?<![\w.#-])(?:\d+\.?\d*|\.\d+)m?s\b/g],
  ["a hex colour", /#[0-9a-fA-F]{3,8}\b/g],
  ["a colour function", /\b(?:rgba?|hsla?|hwb|lab|lch|oklab|oklch|color-mix|color)\(/g],
  [
    "a named colour",
    /:\s*[^;{}]*\b(?:white|black|red|green|blue|gr[ae]y|yellow|orange|purple|pink|brown)\b/g,
  ],
];
const zero = (m: string) => /^-?(?:0+\.?0*|\.0+)[a-z]*$/.test(m);

/** Every raw value in one stylesheet's text, as "line: what: value". var() names and @apply's token classes are
 * the tokens themselves, so they're read past. */
export function scanCss(text: string): string[] {
  const found: string[] = [];
  const bare = text.replace(/\/\*[\s\S]*?\*\//g, (c) => c.replace(/[^\n]/g, " "));
  bare.split("\n").forEach((line, i) => {
    const code = line.replace(/@apply[^;]*;?/g, "").replace(/var\(--[\w-]+/g, "var(");
    for (const [what, rule] of CSS) {
      for (const m of code.matchAll(rule)) {
        const v = m[0].trim();
        if (what === "a raw length" || what === "a raw duration") if (zero(v)) continue;
        found.push(`${i + 1}: ${what} "${v}": ${line.trim()}`);
      }
    }
  });
  return found;
}

test("no raw lengths, colours or durations in the stylesheets of Clauseo's parts", () => {
  const found: string[] = [];
  let scanned = 0;
  for (const file of readdirSync(cssRoot)) {
    if (!file.endsWith(".css")) continue;
    scanned++;
    for (const f of scanCss(readFileSync(new URL(file, cssRoot), "utf8")))
      found.push(`${file}:${f}`);
  }
  expect(scanned).toBeGreaterThanOrEqual(15);
  expect(found).toEqual([]);
});

test("the stylesheet scan catches what it's for", () => {
  expect(scanCss(".a { padding: 10px var(--s2); }")).toHaveLength(1);
  expect(scanCss(".a { box-shadow: inset 0 1px 0 var(--divide); }")).toHaveLength(1);
  expect(scanCss(".a { height: calc(22px * var(--k)); }")).toHaveLength(1);
  expect(scanCss(".a { transition: opacity 140ms; }")).toHaveLength(1);
  expect(scanCss(".a { transition-delay: .2s; }")).toHaveLength(1);
  expect(scanCss(".a { color: #fff; }")).toHaveLength(1);
  expect(scanCss(".a { background: rgba(0, 0, 0, 0.3); }")).toHaveLength(1);
  expect(scanCss(".a { color: white; }")).toHaveLength(1);
  expect(scanCss(".a { bottom: var(--composer-h, 140px); }")).toHaveLength(1);
  expect(scanCss("@media (max-width: 999px) { .a { order: 1; } }")).toHaveLength(1);
  expect(scanCss(":root { --lead-h: 52px; }")).toHaveLength(1);
  expect(
    scanCss(
      "/* 26 px on 30 px */ .chip-green, .s2 { @apply text-ui bg-green; padding: 0 var(--s2); width: 50%; " +
        "transform: rotate(90deg) scale(1.06); opacity: 0.5; margin: 0; transition-delay: 0s; }",
    ),
  ).toEqual([]);
});

test("the scan catches what it's for", () => {
  expect(scan('<div className="bg-[#fff] p-s2" />')).toHaveLength(1);
  expect(scan('<div className="text-red-500 w-[37px]" />')).toHaveLength(1);
  expect(scan('<div className="[&>p]:mt-s2" />')).toHaveLength(1);
  expect(scan('const c = "#C2362B";')).toHaveLength(1);
  expect(scan("<div style={{ color: 'red' }} />")).toHaveLength(1);
  expect(scan('<p className="mt-12px" />')).toHaveLength(1);
  expect(
    scan('const [a, b] = pages(pb); <a href={`#paper-${id}`} className="p-s2 text-ui" />'),
  ).toEqual([]);
});
