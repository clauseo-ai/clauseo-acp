import { execFileSync } from "node:child_process";
import { createRequire } from "node:module";
import { readFileSync, realpathSync } from "node:fs";
import { dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { expect, test } from "vitest";
import { createAnswer } from "./answer.ts";

// micromark decodes named character references (`&amp;`, `&copy;`) with decode-named-character-reference,
// whose `browser` build sets `innerHTML`: under the app's Trusted Types CSP that throws and unmounts the
// whole React root. ui/patches/ drops the package's `browser` condition (ui/pnpm-workspace.yaml,
// patchedDependencies), so every bundler takes the table-based index.js. Vitest resolves with Node's
// conditions and would never see the browser build, so this resolves it the way a browser bundle does.

/** The file a browser bundle gets for decode-named-character-reference, as micromark's own consumer resolves it. */
function browserBuild(): string {
  const consumer = realpathSync(
    dirname(createRequire(import.meta.url).resolve("mdast-util-from-markdown")),
  );
  const url = execFileSync(
    process.execPath,
    [
      "--conditions=browser",
      "--input-type=module",
      "--eval",
      'process.stdout.write(import.meta.resolve("decode-named-character-reference"))',
    ],
    { cwd: consumer, encoding: "utf8" },
  );
  return fileURLToPath(url);
}

test("the browser build of the entity decoder sets no innerHTML (the pnpm patch is applied)", () => {
  const file = browserBuild();
  expect(file).toMatch(/decode-named-character-reference[\\/]index\.js$/);
  expect(readFileSync(file, "utf8")).not.toMatch(/innerHTML/);
});

test("named character references decode to their characters", () => {
  const [view] = createAnswer("t").update("AT&amp;T, &copy; 2026 and&nbsp;&mdash; &bogus;", false);
  const paragraph = view?.hast.children[0];
  const text =
    paragraph?.type === "element" && paragraph.children[0]?.type === "text"
      ? paragraph.children[0].value
      : "";
  expect(text).toBe("AT&T, © 2026 and\u00a0— &bogus;");
});
