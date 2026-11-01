import { readdirSync, readFileSync } from "node:fs";
import { expect, test } from "vitest";
import { LANGUAGES, ROLES, type Lines } from "./languages.ts";
import { tokenize } from "./worker.ts";

const role = (name: (typeof ROLES)[number]) => ROLES.indexOf(name);
/** The tokens of one role, as text. */
const of = (lines: Lines, name: (typeof ROLES)[number]) =>
  lines.flat().flatMap(([r, text]) => (r === role(name) ? [text.trim()] : []));

test("TypeScript tokens come back by role, and the text is the code unchanged", async () => {
  const code = [
    "// a comment",
    "export async function load(id: string): Promise<Matter> {",
    "  const url = `/matters/${id}`;",
    "  return fetch(url, { retries: 3, strict: true });",
    "}",
  ].join("\n");
  const lines = await tokenize(code, "typescript");
  expect(lines.map((line) => line.map(([, text]) => text).join("")).join("\n")).toBe(code);
  expect(of(lines, "comment").join(" ")).toContain("a comment");
  expect(of(lines, "keyword")).toEqual(
    expect.arrayContaining(["export", "async", "function", "const", "return"]),
  );
  expect(of(lines, "function")).toEqual(expect.arrayContaining(["load", "fetch"]));
  expect(of(lines, "type")).toEqual(expect.arrayContaining(["string", "Promise", "Matter"]));
  expect(of(lines, "constant")).toEqual(expect.arrayContaining(["3", "true"]));
  expect(of(lines, "string").join("")).toContain("/matters/");
});

test("every grammar loads under the JavaScript engine and colours something", async () => {
  const samples: Record<(typeof LANGUAGES)[number], string> = {
    json: '{"court": "SC", "year": 2019}',
    xml: '<case id="1">text</case>',
    html: '<p class="x">Hi</p><script>const a = 1;</script>',
    markdown: "# Heading\n\nSome `code` here.",
    python: "def f(x):\n    return x + 1  # one",
    typescript: "let a: number = 1;",
    bash: 'echo "hi" # say it',
    sql: "SELECT name FROM cases WHERE year = 2019;",
  };
  for (const language of LANGUAGES) {
    const lines = await tokenize(samples[language], language);
    expect(
      lines.flat().some(([r]) => r !== 0),
      language,
    ).toBe(true);
  }
});

test("nothing but the worker imports Shiki, in ui/components or ui/app", () => {
  const importers: string[] = [];
  for (const root of ["../../", "../../../../app/src/"]) {
    const src = new URL(root, import.meta.url);
    for (const file of readdirSync(src, { recursive: true })) {
      if (!/\.(tsx?|mts|cts|mjs)$/.test(file)) continue;
      const text = readFileSync(new URL(file, src), "utf8");
      if (/\bfrom\s+["']shiki|import\(\s*["']shiki|["']@shikijs\//.test(text)) importers.push(file);
    }
  }
  expect(importers).toEqual(["markdown/highlight/worker.ts"]);
});
