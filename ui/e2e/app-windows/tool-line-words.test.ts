// The words tool-line.spec.ts expects are the words the window says: each is a key of words-tools-line.ts or
// words-tools.ts, the line's name once its slots are filled.

import { readFileSync } from "node:fs";
import { expect, test } from "@playwright/test";
import { BROWSER_REASONS, OFF, ON, WEB_ON } from "./tool-line-words.ts";

/** A words file's keys: each quoted or bare key that opens an entry (`"…":` or `On:`). */
const keysOf = (file: string) =>
  [
    ...readFileSync(new URL(`../../components/src/lib/${file}`, import.meta.url), "utf8").matchAll(
      /^\s*(?:"((?:[^"\\]|\\.)*)"|([A-Za-z]+)):\s*null,/gm,
    ),
  ].map((m) => (m[2] ?? JSON.parse(`"${m[1]}"`)) as string);

const line = keysOf("words-tools-line.ts");
const popover = keysOf("words-tools.ts");

test("every line the tool line spec looks for is one the words files have", () => {
  expect(line.length).toBeGreaterThan(5);
  expect(popover.length).toBeGreaterThan(10);
  // the line's name: its sentence template, filled with web search's name and "on"
  expect(line).toContain("{tool}: {state}.");
  expect(line).toContain("Web search");
  expect(line).toContain("on");
  expect("{tool}: {state}.".replace("{tool}", "Web search").replace("{state}", "on")).toBe(WEB_ON);
  expect(popover).toContain(ON);
  expect(popover).toContain(OFF);
  for (const reason of BROWSER_REASONS) expect(line, reason).toContain(reason);
});
