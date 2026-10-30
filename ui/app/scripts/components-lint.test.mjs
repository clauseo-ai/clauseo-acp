// Fails when oxlint stops linting ui/components (or lints a file twice), or when pnpm would skip the
// link's hook on a plain install; why the link: components-link.mjs.

import { lstatSync, readFileSync, readdirSync, realpathSync } from "node:fs";
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

describe("the link's hook runs on every pnpm install", () => {
  it("keeps pnpm's up-to-date shortcut off, so a plain `pnpm -C ui install` remakes the link", () => {
    const yaml = readFileSync(join(UI_DIR, "pnpm-workspace.yaml"), "utf8");
    expect(
      yaml,
      `ui/pnpm-workspace.yaml lacks \`optimisticRepeatInstall: false\`; without it a plain \`pnpm -C ui install\` skips the devPreinstall hook, and the link ui/app/components is not remade (scripts/components-link.mjs)`,
    ).toMatch(/^optimisticRepeatInstall: false\b/m);
  });
});

describe("oxlint's reach into ui/components", () => {
  it("goes through the link ui/app/components", () => {
    expect(isLink(LINK), `ui/app/components is not a link; ${FIX}`).toBe(true);
    expect(realpathSync(LINK), `ui/app/components is not a link to ui/components; ${FIX}`).toBe(
      realpathSync(join(UI_DIR, "components")),
    );
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
