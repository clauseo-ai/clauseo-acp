// pe-subsystem.mjs: fails unless the release's clauseo-guard is a console program, as built and as
// the Windows installers carry it.
//
//   node .github/scripts/pe-subsystem.mjs --console <file>...
//   node .github/scripts/pe-subsystem.mjs --holds <directory> <name>
//
// Codex runs its hook line under PowerShell, which waits for a console program and reads its exit
// code, and not for a GUI one (crates/host/src/bin/clauseo-guard.rs). --console checks that each
// file's PE optional header has Subsystem 3 (IMAGE_SUBSYSTEM_WINDOWS_CUI); release.yml's build leg
// runs it on crates/desktop/binaries/clauseo-guard-<target>.exe as sidecars.mjs placed it. --holds
// checks that a file named <name> (any case) is somewhere under <directory> and is a console
// program; sign-windows runs it on each NSIS installer as 7-Zip extracts it. A file that is not a
// PE, or is cut short, fails.
//
// Node built-ins only. Tests: pe-subsystem.test.mjs next to this file (ci.yml's fast lane).

import { readFile, readdir } from "node:fs/promises";
import { join, resolve } from "node:path";
import { pathToFileURL } from "node:url";

/** IMAGE_SUBSYSTEM_WINDOWS_CUI: a console program. */
export const CONSOLE = 3;

/**
 * The Subsystem of the PE image in `bytes`: the optional header starts 24 bytes after the
 * `PE\0\0` signature, whose offset is at 0x3C, and the field is 68 bytes into it (PE32 and
 * PE32+ alike). Throws on bytes that aren't a PE.
 * @param {Uint8Array} bytes
 * @returns {number}
 */
export function subsystemOf(bytes) {
  const view = Buffer.from(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  if (view.length < 0x40 || view.toString("latin1", 0, 2) !== "MZ") throw new Error("not a PE (no MZ header)");
  const pe = view.readUInt32LE(0x3c);
  if (pe + 24 + 70 > view.length || view.toString("latin1", pe, pe + 4) !== "PE\0\0") {
    throw new Error("not a PE (no PE signature where the header points)");
  }
  return view.readUInt16LE(pe + 24 + 68);
}

/** Every file under `dir`, its subdirectories included. */
async function filesUnder(dir) {
  const out = [];
  for (const entry of await readdir(dir, { withFileTypes: true })) {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) out.push(...(await filesUnder(path)));
    else out.push(path);
  }
  return out;
}

/**
 * The problem with the console program at `file`, or null when it is one.
 * @param {string} file
 */
export async function consoleProblem(file) {
  try {
    const subsystem = subsystemOf(await readFile(file));
    return subsystem === CONSOLE ? null : `${file}: Subsystem ${subsystem}, not ${CONSOLE} (console)`;
  } catch (error) {
    return `${file}: ${error.message}`;
  }
}

/**
 * The problem with `dir` holding the console program `name`, or null when it does.
 * @param {string} dir
 * @param {string} name
 */
export async function holdsProblem(dir, name) {
  const wanted = name.toLowerCase();
  const found = (await filesUnder(dir)).filter((file) => file.toLowerCase().endsWith(`/${wanted}`) ||
    file.toLowerCase().endsWith(`\\${wanted}`));
  if (found.length === 0) return `${dir}: holds no ${name}`;
  for (const file of found) {
    const problem = await consoleProblem(file);
    if (problem) return problem;
  }
  return null;
}

const USAGE = "usage: pe-subsystem.mjs --console <file>... | --holds <directory> <name>";

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const [mode, ...rest] = process.argv.slice(2);
  let problems = [];
  if (mode === "--console" && rest.length > 0) {
    problems = (await Promise.all(rest.map(consoleProblem))).filter(Boolean);
  } else if (mode === "--holds" && rest.length === 2) {
    const problem = await holdsProblem(rest[0], rest[1]).catch((error) => `${rest[0]}: ${error.message}`);
    if (problem) problems = [problem];
  } else {
    console.error(USAGE);
    process.exit(2);
  }
  for (const problem of problems) console.log(`::error::${problem}`);
  if (problems.length > 0) process.exit(1);
  console.log(`ok: ${mode === "--holds" ? `${rest[1]} in ${rest[0]}` : rest.join(" ")}: console programs`);
}
