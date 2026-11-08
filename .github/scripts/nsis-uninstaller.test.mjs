// Tests for nsis-uninstaller.mjs, the uninstaller's capture and substitute passes. Plain Node:
//
//   node --test .github/scripts/nsis-uninstaller.test.mjs

import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const SCRIPT = join(here, "nsis-uninstaller.mjs");

/** A PE32 header with no sections and <body> after it, signed (a PKCS#7 certificate table) or not. */
function pe(body, { signed = false } = {}) {
  const header = Buffer.alloc(0x178);
  header.write("MZ", 0, "latin1");
  header.writeUInt32LE(0x40, 0x3c);
  header.write("PE\0\0", 0x40, "latin1");
  header.writeUInt16LE(0x14c, 0x44);
  header.writeUInt16LE(224, 0x54);
  header.writeUInt16LE(0x10b, 0x58);
  header.writeUInt32LE(16, 0x58 + 92);
  const file = Buffer.concat([header, Buffer.from(body)]);
  if (!signed) {
    return file;
  }
  const certificate = Buffer.alloc(16);
  certificate.writeUInt32LE(16, 0);
  certificate.writeUInt16LE(0x0200, 4);
  certificate.writeUInt16LE(0x0002, 6);
  file.writeUInt32LE(file.length, 0x58 + 96 + 4 * 8);
  file.writeUInt32LE(certificate.length, 0x58 + 96 + 4 * 8 + 4);
  return Buffer.concat([file, certificate]);
}

const run = (...args) => spawnSync(process.execPath, [SCRIPT, ...args], { encoding: "utf8" });

test("capture copies the uninstaller once, and refuses a file that is not a PE", () => {
  const dir = mkdtempSync(join(tmpdir(), "nsis-uninstaller-"));
  try {
    const built = join(dir, "nst1.tmp");
    const dest = join(dir, "captured", "currentUser", "uninstall.exe");
    writeFileSync(built, pe("uninstaller"));
    assert.equal(run("capture", dest, built).status, 0);
    assert.deepEqual(readFileSync(dest), readFileSync(built));
    // A second capture to the same path is a mix-up between bundles.
    const again = run("capture", dest, built);
    assert.equal(again.status, 1);
    assert.match(again.stderr, /already there/);
    writeFileSync(built, "not a PE file");
    const other = join(dir, "captured", "perMachine", "uninstall.exe");
    assert.equal(run("capture", other, built).status, 1);
    assert.equal(existsSync(other), false);
    assert.equal(run("capture", dest).status, 2);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("substitute puts the signed copy in place, says whether this build matched the capture, and refuses an unsigned one", () => {
  const dir = mkdtempSync(join(tmpdir(), "nsis-uninstaller-"));
  try {
    const captured = join(dir, "captured.exe");
    const signed = join(dir, "signed.exe");
    const built = join(dir, "nst2.tmp");
    writeFileSync(captured, pe("uninstaller"));
    writeFileSync(built, pe("uninstaller"));
    // Unsigned: refused, and the uninstaller makensis built is left as it was.
    writeFileSync(signed, pe("uninstaller"));
    assert.equal(run("substitute", signed, captured, built).status, 1);
    assert.deepEqual(readFileSync(built), pe("uninstaller"));
    assert.equal(existsSync(`${signed}.substituted`), false);
    writeFileSync(signed, pe("uninstaller", { signed: true }));
    const done = run("substitute", signed, captured, built);
    assert.equal(done.status, 0, done.stderr);
    assert.deepEqual(readFileSync(built), readFileSync(signed));
    assert.equal(readFileSync(`${signed}.substituted`, "utf8"), "identical\n");
    // A second build that came out different still gets the signed copy, and says so.
    writeFileSync(built, pe("uninstaller, built again"));
    assert.equal(run("substitute", signed, captured, built).status, 0);
    assert.deepEqual(readFileSync(built), readFileSync(signed));
    assert.equal(readFileSync(`${signed}.substituted`, "utf8"), "differs\n");
    assert.equal(run("substitute", signed, join(dir, "missing.exe"), built).status, 1);
    assert.equal(run("substitute", signed, captured).status, 2);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
