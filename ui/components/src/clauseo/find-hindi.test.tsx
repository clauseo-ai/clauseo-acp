// @vitest-environment happy-dom
// D-069: what the Hindi room left in English. Find draws no native clear button ("cancel", WebKit's English, under
// display: none in base.css) and asks for no autocorrect, whose bubble under the field dismisses with an English
// "cancel" too; and the API key field's word is Hindi in both its places (first launch and Connectors), keeping
// "API key" as the term the rest of the Hindi uses.
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, expect, test } from "vitest";
import { HI as CONNECTORS } from "../lib/words-connectors.hi.ts";
import { HI as FIRST_LAUNCH } from "../lib/words-first-launch.hi.ts";
import { Rail } from "./rail.tsx";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement | null = null;
let root: Root | null = null;
afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  root = null;
});

test("Find asks for no spelling check, autocorrect or capitals", async () => {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  const acme = { name: "Acme v. Kestrel", status: "idle" as const, note: "", hint: "" };
  await act(async () =>
    root?.render(
      <Rail open groups={[{ when: "Today", matters: [acme] }]} find={{ all: [], recent: [] }} />,
    ),
  );
  const input = document.querySelector<HTMLInputElement>(".room-find input");
  expect(input?.type).toBe("search");
  expect(input?.getAttribute("spellcheck")).toBe("false");
  expect(input?.getAttribute("autocorrect")).toBe("off");
  expect(input?.getAttribute("autocapitalize")).toBe("off");
});

test("the search field's native clear button isn't drawn, so it isn't read out", () => {
  const base = readFileSync(
    join(dirname(fileURLToPath(import.meta.url)), "../styles/base.css"),
    "utf8",
  );
  const rule = /input::-webkit-search-cancel-button\s*{([^}]*)}/.exec(base)?.[1] ?? "";
  expect(rule).toMatch(/display:\s*none/);
});

test("the API key field's word is Hindi, keeping the term", () => {
  for (const words of [FIRST_LAUNCH, CONNECTORS]) {
    expect(words["API key"]).toBe("API key डालें");
    expect(words["API key"]).toMatch(/[\u0900-\u097f]/);
  }
});
