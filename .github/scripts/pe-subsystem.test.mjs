// Tests for pe-subsystem.mjs, on small PE headers built here. Plain Node, no install, no network.
//
//   node --test .github/scripts/pe-subsystem.test.mjs
//
// ci.yml's fast lane runs it beside the other scripts' tests.

import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { after, test } from "node:test";
import { fileURLToPath } from "node:url";
import { CONSOLE, consoleProblem, holdsProblem, subsystemOf } from "./pe-subsystem.mjs";

const SCRIPT = join(dirname(fileURLToPath(import.meta.url)), "pe-subsystem.mjs");
const scratch = await mkdtemp(join(tmpdir(), "pe-subsystem-test-"));
after(() => rm(scratch, { recursive: true, force: true }));

/** A PE header with `subsystem`: MZ, the PE signature at 0x80, a COFF header, an optional header. */
function makePe(subsystem, { plus = true } = {}) {
  const bytes = Buffer.alloc(0x200);
  bytes.write("MZ", 0, "latin1");
  bytes.writeUInt32LE(0x80, 0x3c);
  bytes.write("PE\0\0", 0x80, "latin1");
  bytes.writeUInt16LE(plus ? 0x20b : 0x10b, 0x80 + 24);
  bytes.writeUInt16LE(subsystem, 0x80 + 24 + 68);
  return bytes;
}

test("the Subsystem is read from the optional header, PE32 and PE32+", () => {
  assert.equal(subsystemOf(makePe(3)), CONSOLE);
  assert.equal(subsystemOf(makePe(2)), 2);
  assert.equal(subsystemOf(makePe(3, { plus: false })), 3);
});

test("bytes that aren't a PE, or are cut short, throw", () => {
  assert.throws(() => subsystemOf(Buffer.from("not a program")), /not a PE/);
  const noSignature = makePe(3);
  noSignature.write("XX", 0x80, "latin1");
  assert.throws(() => subsystemOf(noSignature), /not a PE/);
  assert.throws(() => subsystemOf(makePe(3).subarray(0, 0x90)), /not a PE/);
});

test("--console passes a console program and names a GUI one", async () => {
  const good = join(scratch, "clauseo-guard-x86_64-pc-windows-msvc.exe");
  const gui = join(scratch, "gui.exe");
  await writeFile(good, makePe(3));
  await writeFile(gui, makePe(2));
  assert.equal(await consoleProblem(good), null);
  assert.match(await consoleProblem(gui), /Subsystem 2, not 3/);
  assert.match(await consoleProblem(join(scratch, "missing.exe")), /missing\.exe/);
  const ok = spawnSync(process.execPath, [SCRIPT, "--console", good], { encoding: "utf8" });
  assert.equal(ok.status, 0, ok.stdout + ok.stderr);
  const bad = spawnSync(process.execPath, [SCRIPT, "--console", good, gui], { encoding: "utf8" });
  assert.equal(bad.status, 1);
  assert.match(bad.stdout, /::error::.*gui\.exe/);
});

test("--holds finds the program anywhere in the extracted installer, any case, and checks it", async () => {
  const dir = join(scratch, "extracted");
  await mkdir(join(dir, "$INSTDIR"), { recursive: true });
  await writeFile(join(dir, "$INSTDIR", "Clauseo-Guard.EXE"), makePe(3));
  assert.equal(await holdsProblem(dir, "clauseo-guard.exe"), null);
  const empty = join(scratch, "empty");
  await mkdir(empty);
  assert.match(await holdsProblem(empty, "clauseo-guard.exe"), /holds no clauseo-guard\.exe/);
  const gui = join(scratch, "gui-installer");
  await mkdir(gui);
  await writeFile(join(gui, "clauseo-guard.exe"), makePe(2));
  assert.match(await holdsProblem(gui, "clauseo-guard.exe"), /Subsystem 2/);
  const run = spawnSync(process.execPath, [SCRIPT, "--holds", empty, "clauseo-guard.exe"], { encoding: "utf8" });
  assert.equal(run.status, 1);
  const usage = spawnSync(process.execPath, [SCRIPT, "--holds", dir], { encoding: "utf8" });
  assert.equal(usage.status, 2);
  assert.match(usage.stderr, /^usage: /);
});
