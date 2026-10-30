// One English string, one Hindi. The interface's words are split across several files, and a key that appears in two
// of them must say the same Hindi in both, or one screen reads differently from the next. This reads every words file
// as text, collects the entries of its dictionaries, "key": "value" and key: "value", and fails on a key given two
// different Hindi strings.
import { expect, test } from "vitest";

const DEVA = /[\u0900-\u097F]/;

/** A JavaScript string literal's text, its escapes undone. */
const unescape = (s: string) =>
  s
    .replace(/\\u([0-9a-fA-F]{4})/g, (_, h: string) => String.fromCharCode(parseInt(h, 16)))
    .replace(/\\(.)/g, "$1");

/** The dictionary entries in a file's text: an English key, quoted or bare, its colon right after it as the formatter
 * writes an object's entries, then a Hindi string, on the same line or the next. A ternary's " : " is not an entry. */
function entriesOf(src: string): [string, string][] {
  const out: [string, string][] = [];
  const entry = /(?:"((?:[^"\\\n]|\\.)*)"|\b([A-Za-z_$][\w$]*)):\s*"((?:[^"\\\n]|\\.)*)"/g;
  for (const m of src.matchAll(entry)) {
    const key = unescape(m[1] ?? m[2] ?? "");
    const value = unescape(m[3] ?? "");
    if (DEVA.test(value) && !DEVA.test(key)) out.push([key, value]);
  }
  return out;
}

const FILES: Record<string, string> = import.meta.glob<string>(["./words*.ts", "!./*.test.ts"], {
  query: "?raw",
  import: "default",
  eager: true,
});

test("the parser finds quoted and bare keys, across a line break, and skips a ternary and English values", () => {
  const planted = [
    "const HI = {",
    '  "Show fewer": "कम दिखाएँ",',
    '  Citations: "उद्धृत निर्णय",',
    '  "A long one {n}":',
    '    "एक लंबा {n}",',
    '  "Plain English": "Plain English",',
    "};",
    'const x = lang === "hi" ? "a" : "हिंदी";',
  ].join("\n");
  expect(entriesOf(planted)).toEqual([
    ["Show fewer", "कम दिखाएँ"],
    ["Citations", "उद्धृत निर्णय"],
    ["A long one {n}", "एक लंबा {n}"],
  ]);
});

test("every words file is read, and a fair number of entries with them", () => {
  const names = Object.keys(FILES);
  expect(names).toContain("./words.ts");
  expect(names).toContain("./words-reader.ts");
  expect(names.some((n) => n.endsWith(".test.ts"))).toBe(false);
  const n = Object.values(FILES).reduce((sum, src) => sum + entriesOf(src).length, 0);
  expect(n).toBeGreaterThan(200);
});

/** Each key given more than one Hindi string, with the files that say each. */
function repeats(files: Record<string, string>): Record<string, Record<string, string[]>> {
  const seen = new Map<string, Map<string, string[]>>();
  for (const [file, src] of Object.entries(files)) {
    for (const [key, hi] of entriesOf(src)) {
      const byHindi = seen.get(key) ?? new Map<string, string[]>();
      byHindi.set(hi, [...(byHindi.get(hi) ?? []), file]);
      seen.set(key, byHindi);
    }
  }
  const out: Record<string, Record<string, string[]>> = {};
  for (const [key, byHindi] of seen) if (byHindi.size > 1) out[key] = Object.fromEntries(byHindi);
  return out;
}

test("a key that appears in two words files says the same Hindi in both", () => {
  expect(repeats(FILES)).toEqual({});
  // a key given two Hindi strings is caught
  const planted = {
    "./words-a.ts": '"Citations": "उद्धृत निर्णय",',
    "./words-b.ts": 'Citations: "उद्धरण",',
  };
  expect(Object.keys(repeats(planted))).toEqual(["Citations"]);
});
