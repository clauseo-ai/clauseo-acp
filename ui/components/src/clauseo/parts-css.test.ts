import { readdirSync, readFileSync } from "node:fs";
import { expect, test } from "vitest";

const here = new URL("./", import.meta.url);

test("parts.css imports every stylesheet of Clauseo's parts, once", () => {
  const sheets = readdirSync(here).filter((f) => f.endsWith(".css") && f !== "parts.css");
  const imported = [
    ...readFileSync(new URL("parts.css", here), "utf8").matchAll(/@import "\.\/([\w-]+\.css)";/g),
  ].map((m) => m[1]);
  expect(sheets.length).toBeGreaterThan(10);
  expect([...imported].sort((a, b) => (a ?? "").localeCompare(b ?? ""))).toEqual(
    sheets.sort((a, b) => a.localeCompare(b)),
  );
});
