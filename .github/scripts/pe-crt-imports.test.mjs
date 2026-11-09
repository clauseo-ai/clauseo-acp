// Tests for pe-crt-imports.mjs, on small PE files built here, and for the Windows rustflags that
// release.yml and setup-exe-repro.yml must share. Plain Node, no install, no network.
//
//   node --test .github/scripts/pe-crt-imports.test.mjs
//
// ci.yml's fast lane runs it beside latest-json.test.mjs and sidecars.test.mjs.

import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { after, test } from "node:test";
import { fileURLToPath } from "node:url";
import { checkPe, expandPaths, isForbidden, peImports } from "./pe-crt-imports.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const ROOT = join(HERE, "..", "..");
const SCRIPT = join(HERE, "pe-crt-imports.mjs");

const scratch = await mkdtemp(join(tmpdir(), "pe-crt-imports-test-"));
after(() => rm(scratch, { recursive: true, force: true }));

/**
 * A minimal PE: the headers, then one section at RVA 0x1000 (file offset 0x200) holding the
 * import descriptors, the delay-import descriptors and the DLL names.
 * @param {{ plus?: boolean, machine?: number, imports?: string[], delayImports?: string[],
 *   oldDelayForm?: boolean }} [shape] oldDelayForm writes the delay names as VAs (attributes 0)
 */
function makePe({ plus = true, machine = 0x8664, imports = [], delayImports = [], oldDelayForm = false } = {}) {
  const buf = Buffer.alloc(0x400);
  const imageBase = 0x400000;
  buf.writeUInt16LE(0x5a4d, 0); // MZ
  buf.writeUInt32LE(0x40, 0x3c);
  buf.writeUInt32LE(0x00004550, 0x40); // PE\0\0
  const optSize = plus ? 0xf0 : 0xe0;
  buf.writeUInt16LE(machine, 0x44);
  buf.writeUInt16LE(1, 0x46); // one section
  buf.writeUInt16LE(optSize, 0x54);
  const opt = 0x58;
  buf.writeUInt16LE(plus ? 0x20b : 0x10b, opt);
  if (plus) buf.writeBigUInt64LE(BigInt(imageBase), opt + 24);
  else buf.writeUInt32LE(imageBase, opt + 28);
  buf.writeUInt32LE(0x200, opt + 60); // SizeOfHeaders
  buf.writeUInt32LE(16, opt + (plus ? 108 : 92));
  const dirs = opt + (plus ? 112 : 96);
  const section = opt + optSize;
  buf.write(".idata", section, "latin1");
  buf.writeUInt32LE(0x200, section + 8); // VirtualSize
  buf.writeUInt32LE(0x1000, section + 12); // VirtualAddress
  buf.writeUInt32LE(0x200, section + 16); // SizeOfRawData
  buf.writeUInt32LE(0x200, section + 20); // PointerToRawData

  const rva = (offset) => offset - 0x200 + 0x1000;
  const importAt = 0x200;
  const delayAt = importAt + (imports.length + 1) * 20;
  let nameAt = delayAt + (delayImports.length + 1) * 32;
  const name = (dll) => {
    const at = nameAt;
    buf.write(`${dll}\0`, at, "latin1");
    nameAt += dll.length + 1;
    return rva(at);
  };
  imports.forEach((dll, i) => buf.writeUInt32LE(name(dll), importAt + i * 20 + 12));
  delayImports.forEach((dll, i) => {
    const at = delayAt + i * 32;
    buf.writeUInt32LE(oldDelayForm ? 0 : 1, at);
    buf.writeUInt32LE(oldDelayForm ? name(dll) + imageBase : name(dll), at + 4);
  });
  if (imports.length > 0) buf.writeUInt32LE(rva(importAt), dirs + 1 * 8);
  if (delayImports.length > 0) buf.writeUInt32LE(rva(delayAt), dirs + 13 * 8);
  return buf;
}

const CLEAN = ["KERNEL32.dll", "api-ms-win-crt-runtime-l1-1-0.dll", "ucrtbase.dll", "msvcrt.dll"];

test("a clean PE32+ passes, its imports and delay imports read in order", () => {
  const pe = makePe({ imports: CLEAN, delayImports: ["netapi32.dll", "userenv.dll"] });
  assert.deepEqual(peImports(pe), {
    format: "PE32+",
    machine: "x64",
    imports: CLEAN,
    delayImports: ["netapi32.dll", "userenv.dll"],
  });
  const result = checkPe("clean.exe", pe);
  assert.equal(result.ok, true);
  assert.match(result.lines[0], /^clean\.exe: PE32\+ x64, 6 DLLs/);
});

test("an import of VCRUNTIME140.dll fails, naming the file and the DLL", () => {
  const result = checkPe("mcp.exe", makePe({ machine: 0xaa64, imports: [...CLEAN, "VCRUNTIME140.dll"] }));
  assert.equal(result.ok, false);
  assert.deepEqual(result.lines, ["mcp.exe imports VCRUNTIME140.dll, which needs the VC++ Redistributable"]);
});

test("VCRUNTIME140.dll as a delay import only still fails, in either descriptor form", () => {
  for (const oldDelayForm of [false, true]) {
    const pe = makePe({ imports: CLEAN, delayImports: ["vcruntime140.dll"], oldDelayForm });
    assert.deepEqual(peImports(pe).delayImports, ["vcruntime140.dll"]);
    const result = checkPe("setup.exe", pe);
    assert.equal(result.ok, false);
    assert.match(result.lines[0], /^setup\.exe delay-imports vcruntime140\.dll/);
  }
});

test("a PE32 file is read too, and fails on MSVCP140.dll", () => {
  const pe = makePe({ plus: false, machine: 0x14c, imports: ["KERNEL32.dll", "MSVCP140.dll"] });
  assert.equal(peImports(pe).format, "PE32");
  assert.equal(peImports(pe).machine, "x86");
  assert.deepEqual(checkPe("x86.exe", pe).lines, ["x86.exe imports MSVCP140.dll, which needs the VC++ Redistributable"]);
});

test("garbage, an MZ without a PE, and a table pointing past the file all fail", () => {
  assert.equal(checkPe("junk.exe", Buffer.from("not a portable executable at all, just bytes")).ok, false);
  assert.match(checkPe("empty.exe", Buffer.alloc(0)).lines[0], /can't read its imports: not a PE file/);
  const mzOnly = Buffer.alloc(0x100);
  mzOnly.writeUInt16LE(0x5a4d, 0);
  mzOnly.writeUInt32LE(0x80, 0x3c);
  assert.match(checkPe("mz.exe", mzOnly).lines[0], /no PE signature/);
  const pointsAway = makePe({ imports: CLEAN });
  pointsAway.writeUInt32LE(0x9000, 0x58 + 112 + 8);
  assert.match(checkPe("away.exe", pointsAway).lines[0], /in no section's file data/);
  const truncated = makePe({ imports: CLEAN }).subarray(0, 0x210);
  assert.equal(checkPe("short.exe", truncated).ok, false);
});

test("the rules: the VC++ runtime and debug CRTs fail; the UCRT and Windows' own DLLs pass", () => {
  for (const dll of [
    "VCRUNTIME140.dll", "vcruntime140_1.dll", "vcruntime140d.dll", "MSVCP140.dll", "msvcp140_atomic_wait.dll",
    "msvcp140d.dll", "msvcr120.dll", "vccorlib140.dll", "concrt140.dll", "vcomp140.dll", "vcamp140.dll",
    "mfc140u.dll", "mfcm140.dll", "ucrtbased.dll",
  ]) {
    assert.equal(isForbidden(dll), true, dll);
  }
  for (const dll of [
    "api-ms-win-crt-runtime-l1-1-0.dll", "API-MS-WIN-CRT-HEAP-L1-1-0.DLL", "ucrtbase.dll", "msvcrt.dll",
    "msvcp_win.dll", "mfcore.dll", "kernel32.dll", "ntdll.dll", "WebView2Loader.dll",
  ]) {
    assert.equal(isForbidden(dll), false, dll);
  }
});

test("the command line: directories give their .exe and .dll files; a failure exits 1", async () => {
  const dir = join(scratch, "cli");
  await mkdir(join(dir, "sub"), { recursive: true });
  await writeFile(join(dir, "Clauseo.exe"), makePe({ imports: CLEAN }));
  await writeFile(join(dir, "sub", "helper.DLL"), makePe({ imports: CLEAN }));
  await writeFile(join(dir, "notes.txt"), "not checked");
  assert.deepEqual(await expandPaths([dir]), [join(dir, "Clauseo.exe"), join(dir, "sub", "helper.DLL")]);

  const run = (...args) => spawnSync(process.execPath, [SCRIPT, ...args], { encoding: "utf8" });
  const clean = run(dir);
  assert.equal(clean.status, 0, clean.stdout + clean.stderr);
  assert.match(clean.stdout, /2 files \(1 DLLs\): none imports the VC\+\+ runtime/);

  await writeFile(join(dir, "sub", "bad.exe"), makePe({ imports: ["VCRUNTIME140.dll"] }));
  const bad = run(dir);
  assert.equal(bad.status, 1);
  assert.match(bad.stdout, /::error::.*bad\.exe imports VCRUNTIME140\.dll/);

  assert.equal(run(join(scratch, "missing.exe")).status, 1);
  assert.equal(run(join(dir, "notes.txt")).status, 1);
  assert.equal(run().status, 2);
});

/** The quoted rustflags or RUSTFLAGS on the first line of `text` matching `line`. */
function flagsOf(text, line) {
  const match = text.split("\n").find((l) => line.test(l));
  assert.ok(match, `no line matching ${line}`);
  const value = /rustflags: "([^"]*)"|RUSTFLAGS: "([^"]*)"/.exec(match);
  assert.ok(value, `no rustflags on ${match.trim()}`);
  return value[1] ?? value[2];
}

test("setup-exe-repro.yml's RUSTFLAGS are byte for byte release.yml's Windows rows'", async () => {
  const release = await readFile(join(ROOT, ".github/workflows/release.yml"), "utf8");
  const repro = await readFile(join(ROOT, ".github/workflows/setup-exe-repro.yml"), "utf8");
  const reproFlags = flagsOf(repro, /^ {2}RUSTFLAGS: /);
  assert.match(reproFlags, /-C target-feature=\+crt-static/);
  for (const target of ["x86_64-pc-windows-msvc", "aarch64-pc-windows-msvc"]) {
    const row = new RegExp(`^ {10}- \\{ target: ${target}, runner: [^,]+, tier:`);
    assert.equal(flagsOf(release, row), reproFlags, target);
  }
});

test("crt-static is on every Windows row of release.yml and on no other", async () => {
  const release = await readFile(join(ROOT, ".github/workflows/release.yml"), "utf8");
  const rows = release.split("\n").filter((l) => /^ {10}- \{ target: .*rustflags: /.test(l));
  assert.ok(rows.length >= 7, `only ${rows.length} rows with rustflags`);
  for (const row of rows) {
    const windows = /target: [a-z0-9_]+-pc-windows-msvc,/.test(row);
    assert.equal(/\+crt-static/.test(row), windows, row.trim());
  }
  assert.match(release, /^ {6}RUSTFLAGS: \$\{\{ matrix\.rustflags \}\}$/m);
});

test("release.yml checks the VC++ runtime imports right before the windows-exe upload", async () => {
  const release = await readFile(join(ROOT, ".github/workflows/release.yml"), "utf8");
  const check = release.indexOf("node .github/scripts/pe-crt-imports.mjs");
  const upload = release.indexOf("name: windows-exe-${{ matrix.target }}");
  assert.ok(check > 0 && upload > check, "the check comes before the upload");
  const between = release.slice(check, upload);
  assert.equal((between.match(/^ {6}- (uses|name):/gm) ?? []).length, 1, "only the upload step in between");
  assert.match(between, /if: runner\.os == 'Windows'/);
});
