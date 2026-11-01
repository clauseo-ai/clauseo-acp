// axe-core under Tauri's freezePrototype (tauri.conf.json, `Object.freeze(Object.prototype)` before any page script):
// window.__clauseoAxe() loads axe as one of Vite's ES module chunks, which run in strict mode. This loads axe-core's
// main file, as the bundle takes it, in a realm whose Object.prototype is frozen, in strict mode, and runs it over a
// page. Without ui/patches/axe-core@4.14.0.patch its Color class assigns `toString` on its prototype and throws.
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import vm from "node:vm";
import { Window } from "happy-dom";
import type { AxeResults } from "axe-core";
import { expect, test } from "vitest";

const require = createRequire(import.meta.url);
const PATCHED = readFileSync(require.resolve("axe-core"), "utf8");

/** axe's main file as published, before the patch: the assignment the patch turns into a definition. */
function unpatched(source: string): string {
  const start = source.indexOf("            // Clauseo: defined, not assigned");
  const end = source.indexOf("            });", start) + "            });".length;
  expect(start).toBeGreaterThan(0);
  return (
    source.slice(0, start) +
    [
      "            _Color.prototype[name] = function() {",
      "              for (var _len6 = arguments.length, args = new Array(_len6), _key6 = 0; _key6 < _len6; _key6++) {",
      "                args[_key6] = arguments[_key6];",
      "              }",
      "              return func.apply(void 0, [ this ].concat(args));",
      "            };",
    ].join("\n") +
    source.slice(end)
  );
}

/** Loads `source` as strict code in a new realm whose Object.prototype is frozen, over a page with a nameless button,
 * and runs axe there. */
async function runFrozen(source: string): Promise<AxeResults> {
  const page = new Window({ url: "https://tauri.localhost/" });
  page.document.documentElement.lang = "en";
  page.document.title = "Clauseo";
  const main = page.document.createElement("main");
  main.append(page.document.createElement("button"));
  page.document.body.append(main);
  const realm = vm.createContext({ window: page, setTimeout, clearTimeout });
  vm.runInContext("Object.freeze(Object.prototype);", realm);
  try {
    vm.runInContext(`"use strict";\n${source}`, realm, { filename: "axe.js" });
    const axe = (page as unknown as { axe: typeof import("axe-core") }).axe;
    // happy-dom's Document, typed apart from the DOM's
    return await axe.run(page.document as unknown as Document, { resultTypes: ["violations"] });
  } finally {
    await page.happyDOM.close();
  }
}

test("patched axe-core loads and runs where Object.prototype is frozen, in strict code", async () => {
  const results = await runFrozen(PATCHED);
  expect(results.violations.map((v) => v.id)).toContain("button-name");
});

test("unpatched, the same load throws: the override mistake on Color's toString", async () => {
  expect(PATCHED).toContain("Object.defineProperty(_Color.prototype, name, {");
  await expect(runFrozen(unpatched(PATCHED))).rejects.toThrow(/read.only property 'toString'/);
});
