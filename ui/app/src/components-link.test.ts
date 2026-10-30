import { lstatSync, realpathSync } from "node:fs";
import { expect, test } from "vitest";

// oxlint refuses a path with "..", so the `lint:types` script cannot name ../components. ui/app/components is a link
// to it instead: `oxlint --type-aware` walks into it with this package's .oxlintrc.json, and `cargo xtask check`
// names a finding there as ui/app/components/src/<file>. A checkout that makes the link a plain file (git on Windows
// without symlink support) would lint nothing in ui/components without a word, so this fails instead.
test("ui/app/components is a link to ui/components", () => {
  const link = new URL("../components", import.meta.url);
  expect(lstatSync(link).isSymbolicLink()).toBe(true);
  expect(realpathSync(link)).toBe(realpathSync(new URL("../../components", import.meta.url)));
});
