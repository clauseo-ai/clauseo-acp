// pe-crt-imports.mjs: fails when a Windows exe or DLL we ship imports the VC++ runtime
// (ADR 0035, "Sidecars", "The C runtime").
//
//   node .github/scripts/pe-crt-imports.mjs [--app-local-ok[=<dir>]]... <file-or-directory>...
//
// A directory is searched, with its subdirectories, for *.exe, *.dll, *.node and *.pyd (a Node
// addon and a Python extension module are DLLs); a file is checked whatever its name. A clean
// Windows has no VC++ Redistributable, so an exe importing VCRUNTIME140.dll fails to start there
// (0xC0000135); our runners, which all have the C++ build tools, never see it. Every Rust binary
// we build is linked with -C target-feature=+crt-static (release.yml's matrix and
// setup-exe-repro.yml). release.yml's `build` job runs this over every PE it uploads in
// windows-exe-<target>, fetched ones included, and every DLL placed for bundling; `sign-windows`
// runs it over each NSIS installer and everything 7-Zip extracts from it.
//
// For each file it reads the import table and the delay-import table (PE32 and PE32+, any
// machine) and fails, naming the file and the DLL, on a DLL CRT_RULES forbids. A file that is
// not a PE, or whose tables can't be read, fails too.
//
// --app-local-ok, repeatable, lets a forbidden import pass when the DLL it names ships with the
// app: a file of that name (any case) directly in the importer's folder, or directly in a folder
// given as --app-local-ok=<dir> (each must exist; none is searched recursively). The first match
// wins, and it must itself be a PE. Python needs it: CPython ships vcruntime140.dll and
// vcruntime140_1.dll in its top folder beside python.exe; the .pyd files in its DLLs/ folder
// import VCRUNTIME140.dll and load into a process that already has it; Python wheels ship a
// hashed msvcp140-<hash>.dll in their <pkg>.libs/ folder. Those imports resolve on a clean
// Windows because the app ships the DLL. The DLL found is checked by the same rules, its own
// forbidden imports resolved from its own folder, and each app-local resolution prints a note
// line. release.yml never passes the option: its own shipped files must be clean.
//
// Why the Universal CRT (api-ms-win-crt-*, ucrtbase.dll) is allowed: it is an OS component on
// Windows 10 and later, so those imports resolve on a clean machine; only the VC++ runtime
// (VCRUNTIME, MSVCP and the rest below) needs the Redistributable. No doc names a minimum
// Windows for the app; the sandbox needs Windows 10 22H2, build 19045 (ADR 0049, tier C), and
// firm-it.md's Intune policy targets "Windows 10 and later", both past the UCRT's arrival.
// Clauseo.exe keeps importing api-ms-win-crt-* even under crt-static: tauri-build calls
// static_vcruntime::metabuild() (tauri-build 2.7.3 src/lib.rs, when staticVCRuntime is on, the
// default), whose link args /NODEFAULTLIB:libucrt.lib /DEFAULTLIB:ucrt.lib (static_vcruntime
// 3.0.0 src/lib.rs) keep the UCRT dynamic. msvcrt.dll is Windows' own CRT, a KnownDLL.
//
// Node built-ins only. Tests: pe-crt-imports.test.mjs next to this file (ci.yml's fast lane).

import { readFile, readdir, stat } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { pathToFileURL } from "node:url";

/**
 * Which imported DLLs pass, matched against the lowercased name: the first rule that matches
 * decides, and a DLL no rule matches passes. To forbid the UCRT too, flip its `allowed`.
 * The VC++ runtime's stems want a digit after them, so Windows' own msvcrt.dll, msvcp_win.dll
 * and mfcore.dll (Media Foundation) don't match, while vcruntime140_1.dll, msvcp140_atomic_wait.dll
 * and the debug `d` forms (vcruntime140d.dll, msvcp140d.dll) do. msvcr<NN> is the runtime before
 * 2015; ucrtbased.dll is the debug UCRT, which only the Windows SDK installs.
 */
export const CRT_RULES = [
  { pattern: /^api-ms-win-crt-/, allowed: true },
  { pattern: /^ucrtbase\.dll$/, allowed: true },
  { pattern: /^msvcrt\.dll$/, allowed: true },
  { pattern: /^ucrtbased\.dll$/, allowed: false },
  { pattern: /^vcruntime\d/, allowed: false },
  { pattern: /^msvcp\d/, allowed: false },
  { pattern: /^msvcr\d/, allowed: false },
  { pattern: /^vccorlib\d/, allowed: false },
  { pattern: /^concrt\d/, allowed: false },
  { pattern: /^vcomp\d/, allowed: false },
  { pattern: /^vcamp\d/, allowed: false },
  { pattern: /^mfcm?\d/, allowed: false },
];

/** Whether CRT_RULES forbid a DLL name (any case). */
export function isForbidden(dll) {
  const name = dll.toLowerCase();
  const rule = CRT_RULES.find(({ pattern }) => pattern.test(name));
  return rule !== undefined && !rule.allowed;
}

const MACHINES = { 0x14c: "x86", 0x8664: "x64", 0xaa64: "arm64", 0x1c4: "arm", 0xa641: "arm64ec" };
const IMPORT_DIR = 1;
const DELAY_IMPORT_DIR = 13;
// Far more descriptors than any real exe has; a table without its terminator stops here.
const MAX_DESCRIPTORS = 4096;
const MAX_NAME = 512;

/**
 * The DLL names a PE imports, statically and delay-loaded. Throws when the buffer is not a PE
 * or a table points outside the file.
 * @param {Buffer} buf the whole file
 * @returns {{ format: "PE32" | "PE32+", machine: string, imports: string[],
 *   delayImports: string[] }}
 */
export function peImports(buf) {
  const u16 = (at) => check(at, 2) && buf.readUInt16LE(at);
  const u32 = (at) => check(at, 4) && buf.readUInt32LE(at);
  function check(at, size) {
    if (!Number.isInteger(at) || at < 0 || at + size > buf.length) {
      throw new Error(`truncated: a read at 0x${at.toString(16)} is past the file's end`);
    }
    return true;
  }

  if (buf.length < 0x40 || u16(0) !== 0x5a4d) throw new Error("not a PE file: no MZ header");
  const pe = u32(0x3c);
  if (u32(pe) !== 0x00004550) throw new Error("not a PE file: no PE signature");
  const machineId = u16(pe + 4);
  const sectionCount = u16(pe + 6);
  const optSize = u16(pe + 20);
  const opt = pe + 24;
  const magic = u16(opt);
  if (magic !== 0x10b && magic !== 0x20b) {
    throw new Error(`not a PE file: optional header magic 0x${magic.toString(16)}`);
  }
  const plus = magic === 0x20b;
  const imageBase = plus ? Number(check(opt + 24, 8) && buf.readBigUInt64LE(opt + 24)) : u32(opt + 28);
  const sizeOfHeaders = u32(opt + 60);
  const dirCount = u32(opt + (plus ? 108 : 92));
  const dirs = opt + (plus ? 112 : 96);
  if (dirs + Math.min(dirCount, 16) * 8 > opt + optSize) {
    throw new Error("data directories overrun the optional header");
  }

  const sections = [];
  for (let i = 0; i < sectionCount; i++) {
    const at = opt + optSize + i * 40;
    sections.push({ va: u32(at + 12), virtualSize: u32(at + 8), rawSize: u32(at + 16), raw: u32(at + 20) });
  }
  /** The file offset of an RVA, from its section (or the headers); throws when none holds it. */
  function offset(rva) {
    if (!Number.isInteger(rva) || rva < 0) throw new Error(`bad RVA ${rva}`);
    if (rva < sizeOfHeaders) return rva;
    for (const s of sections) {
      if (rva >= s.va && rva < s.va + Math.max(s.virtualSize, s.rawSize) && rva - s.va < s.rawSize) {
        return s.raw + (rva - s.va);
      }
    }
    throw new Error(`RVA 0x${rva.toString(16)} is in no section's file data`);
  }
  function cstring(rva) {
    const start = offset(rva);
    const end = buf.indexOf(0, start);
    const name = end < 0 || end - start > MAX_NAME ? "" : buf.toString("latin1", start, end);
    if (!/^[\x21-\x7e]+$/.test(name)) throw new Error(`unreadable DLL name at RVA 0x${rva.toString(16)}`);
    return name;
  }
  function dir(index) {
    return index < dirCount ? u32(dirs + index * 8) : 0;
  }
  /** The names of a descriptor table, `size` bytes each, stopping at the all-zero terminator. */
  function table(index, size, nameOf) {
    const rva = dir(index);
    if (rva === 0) return [];
    const names = [];
    for (let i = 0; ; i++) {
      if (i === MAX_DESCRIPTORS) throw new Error(`directory ${index} has no terminator`);
      const at = offset(rva + i * size);
      check(at, size);
      if (buf.subarray(at, at + size).every((byte) => byte === 0)) return names;
      names.push(cstring(nameOf(at)));
    }
  }

  return {
    format: plus ? "PE32+" : "PE32",
    machine: MACHINES[machineId] ?? `0x${machineId.toString(16)}`,
    // IMAGE_IMPORT_DESCRIPTOR: 20 bytes, the name's RVA at 12.
    imports: table(IMPORT_DIR, 20, (at) => u32(at + 12)),
    // ImgDelayDescr: 32 bytes, attributes at 0 and the name at 4; bit 0 clear is the old form,
    // which holds virtual addresses instead of RVAs.
    delayImports: table(DELAY_IMPORT_DIR, 32, (at) => (u32(at) & 1 ? u32(at + 4) : u32(at + 4) - imageBase)),
  };
}

const WHY = "which needs the VC++ Redistributable";

/** The line of a file that passes: its format, machine, DLL count and the runtime's state. */
function passLine(label, parsed, runtime) {
  const count = parsed.imports.length + parsed.delayImports.length;
  return `${label}: ${parsed.format} ${parsed.machine}, ${count} DLLs, ${runtime}`;
}

/**
 * The problems of one file: a VC++ runtime import each, or the reason its tables can't be read.
 * @param {string} label the file's name in the messages
 * @param {Buffer} buf its bytes
 * @returns {{ ok: boolean, lines: string[] }}
 */
export function checkPe(label, buf) {
  let parsed;
  try {
    parsed = peImports(buf);
  } catch (error) {
    return { ok: false, lines: [`${label}: can't read its imports: ${error.message}`] };
  }
  const bad = [
    ...parsed.imports.filter(isForbidden).map((dll) => `${label} imports ${dll}, ${WHY}`),
    ...parsed.delayImports.filter(isForbidden).map((dll) => `${label} delay-imports ${dll}, ${WHY}`),
  ];
  if (bad.length > 0) return { ok: false, lines: bad };
  return { ok: true, lines: [passLine(label, parsed, "no VC++ runtime")] };
}

/**
 * One file on disk, checked. Without `appLocalDirs` this is checkPe on its bytes. With it (an
 * array, empty for the importer's folder alone), a forbidden import passes when its DLL is found
 * app-local, and that DLL passes the same check; see the header.
 * @param {string} path the file
 * @param {{ appLocalDirs?: string[] }} [options] undefined turns app-local resolution off
 * @returns {Promise<{ ok: boolean, lines: string[], notes: string[] }>} the failure lines, or
 *   the verdict line when ok; notes name each app-local resolution, for printing as notes
 */
export async function checkFile(path, { appLocalDirs } = {}) {
  const buf = await readFile(path);
  if (appLocalDirs === undefined) return { ...checkPe(path, buf), notes: [] };
  return checkAppLocal(path, buf, appLocalDirs, [resolve(path)]);
}

/**
 * checkPe with app-local resolution. `stack` holds the absolute paths on the chain being checked,
 * so a cycle ends at a file already in progress: its own result is reported at its own level.
 */
async function checkAppLocal(label, buf, appLocalDirs, stack) {
  let parsed;
  try {
    parsed = peImports(buf);
  } catch (error) {
    return { ok: false, lines: [`${label}: can't read its imports: ${error.message}`], notes: [] };
  }
  const searched = [dirname(label), ...appLocalDirs];
  const bad = [];
  const notes = [];
  let resolved = 0;
  for (const [verb, dlls] of [["imports", parsed.imports], ["delay-imports", parsed.delayImports]]) {
    for (const dll of dlls.filter(isForbidden)) {
      const found = await findAppLocal(dll, searched);
      if (found === undefined) {
        bad.push(`${label} ${verb} ${dll}, ${WHY}`);
        continue;
      }
      const foundBuf = await readFile(found).catch(() => Buffer.alloc(0));
      try {
        peImports(foundBuf);
      } catch {
        bad.push(`${label} ${verb} ${dll}; ${found} is not a readable PE`);
        continue;
      }
      const note = `${label} ${verb} ${dll}, found at ${found}`;
      const key = resolve(found);
      if (stack.includes(key)) {
        notes.push(note);
        resolved++;
        continue;
      }
      const sub = await checkAppLocal(found, foundBuf, appLocalDirs, [...stack, key]);
      if (sub.ok) {
        notes.push(note, ...sub.notes);
        resolved++;
      } else {
        notes.push(...sub.notes);
        bad.push(`${label} ${verb} ${dll}, found at ${found}, which fails: ${sub.lines.join("; ")}`);
      }
    }
  }
  if (bad.length > 0) return { ok: false, lines: bad, notes };
  const runtime = resolved > 0 ? "VC++ runtime app-local" : "no VC++ runtime";
  return { ok: true, lines: [passLine(label, parsed, runtime)], notes };
}

/**
 * The file named `dll` (any case) in the first of `dirs` that holds one, each folder searched
 * directly, not recursively; undefined when none does.
 * @param {string} dll the DLL's name as imported
 * @param {string[]} dirs the folders, in search order
 * @returns {Promise<string | undefined>}
 */
async function findAppLocal(dll, dirs) {
  const wanted = dll.toLowerCase();
  for (const dir of dirs) {
    const names = await readdir(dir).catch(() => []);
    for (const name of names.sort()) {
      if (name.toLowerCase() !== wanted) continue;
      const path = join(dir, name);
      if (await stat(path).then((s) => s.isFile(), () => false)) return path;
    }
  }
  return undefined;
}

/** The file names a directory's search picks up, any case. */
export const PE_NAME = /\.(exe|dll|node|pyd)$/i;

/**
 * The files to check: each file argument as given, and each directory's *.exe, *.dll, *.node and
 * *.pyd (any case, subdirectories included), sorted. Throws when an argument doesn't exist.
 * @param {string[]} args files and directories
 * @returns {Promise<string[]>}
 */
export async function expandPaths(args) {
  const files = [];
  for (const arg of args) {
    if (!(await stat(arg)).isDirectory()) {
      files.push(arg);
      continue;
    }
    const found = [];
    for (const entry of await readdir(arg, { recursive: true, withFileTypes: true })) {
      if (entry.isFile() && PE_NAME.test(entry.name)) {
        found.push(join(entry.parentPath, entry.name));
      }
    }
    files.push(...found.sort());
  }
  return files;
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const usage = "usage: node .github/scripts/pe-crt-imports.mjs " +
    "[--app-local-ok[=<dir>]]... <file-or-directory>...";
  const args = [];
  let appLocalDirs; // undefined: app-local resolution off
  for (const arg of process.argv.slice(2)) {
    if (arg === "--app-local-ok") {
      appLocalDirs ??= [];
    } else if (arg.startsWith("--app-local-ok=")) {
      (appLocalDirs ??= []).push(arg.slice("--app-local-ok=".length));
    } else {
      args.push(arg);
    }
  }
  if (args.length === 0) {
    console.error(usage);
    process.exit(2);
  }
  for (const dir of appLocalDirs ?? []) {
    if (!(await stat(dir).then((s) => s.isDirectory(), () => false))) {
      console.log(`::error::--app-local-ok=${dir}: not an existing folder`);
      process.exit(1);
    }
  }
  let files;
  try {
    files = await expandPaths(args);
  } catch (error) {
    console.log(`::error::${error.message}`);
    process.exit(1);
  }
  let failed = 0;
  let resolved = 0;
  for (const file of files) {
    let result;
    try {
      result = await checkFile(file, { appLocalDirs });
    } catch (error) {
      result = { ok: false, lines: [`${file}: can't read it: ${error.message}`], notes: [] };
    }
    for (const note of result.notes) console.log(`note: ${note}`);
    for (const line of result.lines) console.log(result.ok ? `ok ${line}` : `::error::${line}`);
    resolved += result.notes.length;
    if (!result.ok) failed++;
  }
  const dlls = files.filter((file) => /\.(dll|node|pyd)$/i.test(file)).length;
  const summary = `${files.length} files (${dlls} DLLs or addons)`;
  if (files.length === 0) {
    console.log(`::error::no .exe, .dll, .node or .pyd in ${args.join(" ")}`);
    process.exit(1);
  }
  if (failed > 0) {
    console.log(`${failed} of ${summary} import the VC++ runtime or can't be read; ` +
      "link with -C target-feature=+crt-static");
    process.exit(1);
  }
  if (resolved > 0) {
    console.log(`${summary}: none imports the VC++ runtime unresolved; ${resolved} app-local (see the notes)`);
  } else {
    console.log(`${summary}: none imports the VC++ runtime`);
  }
}
