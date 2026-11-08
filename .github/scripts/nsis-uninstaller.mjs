// The uninstaller's two passes for Authenticode (S20a, release.yml's sign-windows). makensis builds
// uninstall.exe inside the installer, so it can't be signed as a release artifact the way every
// other PE is. The template's `sign-uninstaller` block (crates/desktop/windows/nsis/installer.nsi)
// runs `$%CLAUSEO_UNINSTALLER_COMMAND% "<the uninstaller>"` through NSIS's `!uninstfinalize`, after
// makensis writes the uninstaller and before it packs it; this script is that command:
//
//   node .github/scripts/nsis-uninstaller.mjs capture <dest> <uninstaller>
//   node .github/scripts/nsis-uninstaller.mjs substitute <signed> <captured> <uninstaller>
//
// A first bundle of each installer captures its uninstaller (a copy at <dest>, which must not exist
// yet); the workflow signs the copies with SignPath's GitHub action, the one route that gives
// SignPath the origin data it requires; a second bundle of the same installer, from the same
// inputs, then substitutes: the signed copy takes the place of the uninstaller makensis has just
// written, and makensis packs that. substitute refuses a <signed> without an Authenticode signature,
// then writes <signed>.substituted: `identical` when the uninstaller it replaced has <captured>'s
// bytes, else `differs` (the second build came out different; the workflow warns, as the signed
// copy is still the uninstaller built from these inputs). Exit status: 0 done, 1 refused, 2 usage.

import { copyFileSync, existsSync, mkdirSync, readFileSync, realpathSync, writeFileSync } from "node:fs";
import { dirname } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { authenticode } from "./latest-json.mjs";

/** Copies <uninstaller> to <dest>. Throws when it is not a PE file or <dest> is already there. */
export function capture(dest, uninstaller) {
  authenticode(readFileSync(uninstaller));
  if (existsSync(dest)) {
    throw new Error(`${dest} is already there: each bundle captures to a path of its own`);
  }
  mkdirSync(dirname(dest), { recursive: true });
  copyFileSync(uninstaller, dest);
}

/**
 * Puts <signed> in place of <uninstaller> and returns `identical` or `differs` (<uninstaller>
 * against <captured>), which it also writes to <signed>.substituted. Throws when <signed> is not a
 * signed PE file or <captured> is missing; <uninstaller> is then left as it was.
 */
export function substitute(signed, captured, uninstaller) {
  if (authenticode(readFileSync(signed)) === null) {
    throw new Error(`${signed} has no Authenticode signature`);
  }
  const same = readFileSync(captured).equals(readFileSync(uninstaller));
  const result = same ? "identical" : "differs";
  copyFileSync(signed, uninstaller);
  writeFileSync(`${signed}.substituted`, `${result}\n`);
  return result;
}

function main(argv) {
  try {
    if (argv[0] === "capture" && argv.length === 3) {
      capture(argv[1], argv[2]);
      process.stdout.write(`nsis-uninstaller: captured the uninstaller at ${argv[1]}\n`);
      return 0;
    }
    if (argv[0] === "substitute" && argv.length === 4) {
      const result = substitute(argv[1], argv[2], argv[3]);
      process.stdout.write(`nsis-uninstaller: substituted ${argv[1]} (this build's uninstaller: ${result} to the capture)\n`);
      return 0;
    }
  } catch (error) {
    process.stderr.write(`nsis-uninstaller: ${error.message}\n`);
    return 1;
  }
  process.stderr.write(
    "usage: node nsis-uninstaller.mjs capture <dest> <uninstaller>\n" +
      "       node nsis-uninstaller.mjs substitute <signed> <captured> <uninstaller>\n",
  );
  return 2;
}

function realHref(path) {
  try {
    return pathToFileURL(realpathSync(path)).href;
  } catch {
    return "";
  }
}
const entry = process.argv[1] === undefined ? "" : realHref(process.argv[1]);
if (entry !== "" && realHref(fileURLToPath(import.meta.url)) === entry) {
  process.exitCode = main(process.argv.slice(2));
}
