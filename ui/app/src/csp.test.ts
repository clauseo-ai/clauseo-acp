import { readdirSync, readFileSync } from "node:fs";
import { expect, test } from "vitest";
import tauriConf from "../../../crates/desktop/tauri.conf.json" with { type: "json" };

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

test("the app's page has no inline script or style", () => {
  const html = readFileSync(new URL("../index.html", import.meta.url), "utf8");
  expect(html).not.toMatch(/<style|style=/);
  for (const m of html.matchAll(/<script([^>]*)>([\s\S]*?)<\/script>/g)) {
    expect(m[1]).toMatch(/\bsrc=/);
    expect(m[2]?.trim()).toBe("");
  }
});
