// Makes ui/app/components, the link through which `oxlint --type-aware` (the `lint:types` script) lints
// ui/components with ui/app's .oxlintrc.json. oxlint refuses a path with `..` and skips node_modules,
// so the script cannot name ../components; and git cannot carry the link, since a checkout without
// symlink support (git on Windows without Developer Mode) writes it as a plain file. So the link is
// ignored by git and made here, by ui/package.json's `pnpm:devPreinstall`, which pnpm runs on every
// `pnpm -C ui install`, up to date or not. On Windows it is a junction, which needs no rights.
// oxlint skips what git ignores; ui/app/.eslintignore, which git does not read, un-ignores the link.
// scripts/components-lint.test.mjs fails when the link is missing or oxlint does not walk into it.

import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { ensureLink } from "./guard.mjs";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");

const done = ensureLink({ root: ROOT, rel: "ui/app/components", target: "../components" });
if (done !== "kept") {
  process.stdout.write(
    `components-link.mjs: ${done} the link ui/app/components -> ../components for oxlint\n`,
  );
}
