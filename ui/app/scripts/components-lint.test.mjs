// The guard over oxlint's reach into ui/components. `lint:types` lints ui/components through the
// link ui/app/components, which `pnpm -C ui install` makes (components-link.mjs) and git ignores;
// ui/app/.eslintignore un-ignores it for oxlint. If the link is missing, or oxlint stops walking
// into it, `oxlint --type-aware` would lint nothing in ui/components without a word, so this fails
// instead. It also fails when a file would be linted twice.

import { lstatSync, readdirSync, realpathSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { scriptCommand } from "./buildtime-lib.mjs";
import { runTool } from "./buildtime-proc.mjs";

const APP_DIR = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const UI_DIR = resolve(APP_DIR, "..");
const LINK = join(APP_DIR, "components");
/** How long `oxlint --debug=files` may take; it lists the files and exits, well under a second. */
const OXLINT_TIMEOUT_MS = 60_000;
/** The extensions of the files in ui/components that oxlint lints. */
const LINTED = /\.(?:[cm]?[jt]sx?)$/;
const FIX =
  "run `pnpm -C ui install`, which makes the link ui/app/components (scripts/components-link.mjs)";

function isLink(path) {
  try {
    return lstatSync(path).isSymbolicLink();
  } catch {
    return false;
  }
}

describe("oxlint's reach into ui/components", () => {
  it("goes through the link ui/app/components", () => {
    expect(isLink(LINK), `ui/app/components is not a link; ${FIX}`).toBe(true);
    expect(realpathSync(LINK), FIX).toBe(realpathSync(join(UI_DIR, "components")));
  });

  it("lints every source file of ui/components, and every file once", async () => {
    const want = readdirSync(join(UI_DIR, "components", "src"), { recursive: true })
      .map((file) => `components/src/${String(file).replaceAll("\\", "/")}`)
      .filter((file) => LINTED.test(file))
      .toSorted();
    expect(want.length).toBeGreaterThan(0);
    const { argv } = scriptCommand(APP_DIR, UI_DIR, "lint:types");
    const result = await runTool([...argv, "--debug=files"], {
      cwd: APP_DIR,
      timeoutMs: OXLINT_TIMEOUT_MS,
    });
    expect(result.timedOut, result.output).toBe(false);
    expect(result.code, result.output).toBe(0);
    const listed = result.output
      .split(/\r?\n/)
      .map((line) => line.trim().replaceAll("\\", "/"))
      .filter((line) => line !== "");
    expect(
      listed.filter((file) => file.startsWith("components/")).toSorted(),
      `oxlint does not lint ui/components through ui/app/components; ${FIX}, and keep \`!/components\` in ui/app/.eslintignore`,
    ).toEqual(want);
    expect(listed.length, "oxlint lists a file twice").toBe(new Set(listed).size);
  });
});
