// The words tool-line.spec.ts expects are the words the window says: each is a key of words-tools.ts once its slots
// are filled ({name} with a plan's name, {managed} with the settings window's "Managed by your organisation").

import { readFileSync } from "node:fs";
import { expect, test } from "@playwright/test";
import { BROWSER_LINES, WEB_ON } from "./tool-line-words.ts";

const words = readFileSync(
  new URL("../../components/src/lib/words-tools.ts", import.meta.url),
  "utf8",
);

/** The words file's keys: each quoted string that opens an entry (`"…":`). */
const keys = [...words.matchAll(/^\s*"((?:[^"\\]|\\.)*)":/gm)].map(
  (m) => JSON.parse(`"${m[1]}"`) as string,
);

const filled = keys.flatMap((k) =>
  ["Claude", "ChatGPT"].map((name) =>
    k.replaceAll("{name}", name).replaceAll("{managed}", "Managed by your organisation"),
  ),
);

test("every line the tool line spec looks for is one the words file has", () => {
  expect(keys.length).toBeGreaterThan(10);
  for (const line of [WEB_ON, ...BROWSER_LINES]) expect(filled, line).toContain(line);
});
