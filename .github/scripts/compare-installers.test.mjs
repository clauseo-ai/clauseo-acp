// Tests for compare-installers.mjs. Plain Node, no install and no 7-Zip:
//
//   node --test .github/scripts/compare-installers.test.mjs
//
// The PE files are built here: a PE32+ with a version resource, a debug directory (a CodeView RSDS
// entry and a REPRO entry) and an optional certificate table. The CLI tests put a stand-in `7z` on
// PATH (a shell script that copies a folder named in the installer file); they are skipped on Windows.

import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { chmodSync, mkdirSync, mkdtempSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { delimiter, dirname, join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { compareTrees, normalizePe, versionResourceRange } from "./compare-installers.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const SCRIPT = join(here, "compare-installers.mjs");
const RAW = 0x200;
const VA = 0x1000;
const CODE = Buffer.alloc(32, 0x90);
const DEBUG_SIZE = 28;

/** One VERSIONINFO node: header, NUL-ended UTF-16 key, padding, value (text or bytes), padding, children. */
function versionNode(key, { text = null, bytes = Buffer.alloc(0), children = [] } = {}) {
  const pad = (b) => Buffer.concat([b, Buffer.alloc((4 - (b.length % 4)) % 4)]);
  const value = text === null ? bytes : Buffer.from(`${text}\0`, "utf16le");
  const head = Buffer.alloc(6);
  head.writeUInt16LE(text === null ? bytes.length : text.length + 1, 2);
  head.writeUInt16LE(text === null ? 0 : 1, 4);
  const body = Buffer.concat([pad(Buffer.concat([head, Buffer.from(`${key}\0`, "utf16le")])), pad(value), ...children.map(pad)]);
  body.writeUInt16LE(body.length, 0);
  return body;
}

/** The VERSIONINFO resource tree for <strings> (name: value), as one .data section's resource bytes. */
function versionResource(strings) {
  const fixed = Buffer.alloc(52);
  fixed.writeUInt32LE(0xfeef04bd, 0);
  const info = versionNode("VS_VERSION_INFO", {
    bytes: fixed,
    children: [
      versionNode("StringFileInfo", {
        children: [
          versionNode("040904B0", {
            children: Object.entries(strings).map(([key, text]) => versionNode(key, { text })),
          }),
        ],
      }),
    ],
  });
  // Three directories of one entry each (16 + 8 bytes), a leaf data entry (16), then the data at 88.
  const dir = (id, target, subdirectory) => {
    const b = Buffer.alloc(24);
    b.writeUInt16LE(1, 14);
    b.writeUInt32LE(id, 16);
    b.writeUInt32LE((target | (subdirectory ? 0x80000000 : 0)) >>> 0, 20);
    return b;
  };
  const entry = Buffer.alloc(16);
  entry.writeUInt32LE(VA + 88, 0);
  entry.writeUInt32LE(info.length, 4);
  return Buffer.concat([dir(16, 24, true), dir(1, 48, true), dir(0x409, 72, false), entry, info]);
}

/**
 * A PE32+ file: MZ and PE headers, one .data section at file offset 0x200 (RVA 0x1000) holding a
 * VERSIONINFO resource (unless <strings> is null), a debug directory with a CodeView RSDS entry (GUID
 * filled with <guid>, age <age>) and a REPRO entry (8 bytes of <repro>), then <code>. <timestamp> is
 * the COFF TimeDateStamp, <debugStamp> the debug entries' timestamps, <checkSum> the optional
 * header's CheckSum. <certificate> (a Buffer) is appended as the certificate table, data directory 4.
 */
function fakePe({
  strings = { ProductName: "Clauseo" },
  code = CODE,
  timestamp = 0x5f000000,
  debugStamp = 0x5e000000,
  checkSum = 0x1234,
  guid = 0x11,
  age = 1,
  repro = 0xaa,
  certificate = null,
} = {}) {
  const resources = strings === null ? Buffer.alloc(0) : versionResource(strings);
  const debugAt = resources.length;
  const debug = Buffer.alloc(56);
  const rsdsAt = debugAt + debug.length;
  const rsds = Buffer.concat([
    Buffer.from("RSDS", "latin1"),
    Buffer.alloc(16, guid),
    Buffer.from([age, 0, 0, 0]),
    Buffer.from("app.pdb\0", "latin1"),
  ]);
  const reproAt = rsdsAt + rsds.length;
  const reproData = Buffer.alloc(8, repro);
  debug.writeUInt32LE(debugStamp, 4);
  debug.writeUInt32LE(2, 12);
  debug.writeUInt32LE(rsds.length, 16);
  debug.writeUInt32LE(RAW + rsdsAt, 24);
  debug.writeUInt32LE(debugStamp, DEBUG_SIZE + 4);
  debug.writeUInt32LE(16, DEBUG_SIZE + 12);
  debug.writeUInt32LE(reproData.length, DEBUG_SIZE + 16);
  debug.writeUInt32LE(RAW + reproAt, DEBUG_SIZE + 24);
  const body = Buffer.concat([resources, debug, rsds, reproData, code]);

  const header = Buffer.alloc(RAW);
  header.write("MZ", 0, "latin1");
  header.writeUInt32LE(0x40, 0x3c);
  header.write("PE\0\0", 0x40, "latin1");
  header.writeUInt16LE(0x8664, 0x44);
  header.writeUInt16LE(1, 0x46);
  header.writeUInt32LE(timestamp, 0x48);
  header.writeUInt16LE(240, 0x54);
  const optional = 0x58;
  header.writeUInt16LE(0x20b, optional);
  header.writeUInt32LE(checkSum, optional + 64);
  header.writeUInt32LE(16, optional + 108);
  if (strings !== null) {
    header.writeUInt32LE(VA, optional + 112 + 2 * 8);
    header.writeUInt32LE(resources.length, optional + 112 + 2 * 8 + 4);
  }
  header.writeUInt32LE(VA + debugAt, optional + 112 + 6 * 8);
  header.writeUInt32LE(debug.length, optional + 112 + 6 * 8 + 4);
  const section = optional + 240;
  header.write(".data", section, "latin1");
  header.writeUInt32LE(body.length, section + 8);
  header.writeUInt32LE(VA, section + 12);
  header.writeUInt32LE(body.length, section + 16);
  header.writeUInt32LE(RAW, section + 20);
  const file = Buffer.concat([header, body]);
  if (certificate === null) {
    return file;
  }
  header.writeUInt32LE(file.length, optional + 112 + 4 * 8);
  header.writeUInt32LE(certificate.length, optional + 112 + 4 * 8 + 4);
  return Buffer.concat([header, body, certificate]);
}

/** A WIN_CERTIFICATE: dwLength, wRevision, wCertificateType, then <body> (a stand-in for the DER), padded to 8. */
function winCertificate(body = Buffer.from("fake PKCS#7 SignedData"), { revision = 0x0200, type = 0x0002 } = {}) {
  const head = Buffer.alloc(8);
  head.writeUInt32LE(8 + body.length, 0);
  head.writeUInt16LE(revision, 4);
  head.writeUInt16LE(type, 6);
  const entry = Buffer.concat([head, body]);
  return Buffer.concat([entry, Buffer.alloc((8 - (entry.length % 8)) % 8)]);
}

/** A fresh temporary folder; the caller removes it. */
function tempDir() {
  return mkdtempSync(join(tmpdir(), "compare-installers-test-"));
}

/** Writes <files> (relative path to a string or Buffer) under <root>, making folders as needed. */
function writeTree(root, files) {
  for (const [path, content] of Object.entries(files)) {
    const full = join(root, path);
    mkdirSync(dirname(full), { recursive: true });
    writeFileSync(full, content);
  }
  return root;
}

/** The first byte offset where <a> and <b> differ. */
function firstByteDifference(a, b) {
  let at = 0;
  while (a[at] === b[at]) {
    at += 1;
  }
  return at;
}

test("normalizePe makes PEs equal that differ only in timestamps, checksum, debug IDs, the signature, or a certificate", () => {
  const base = normalizePe(fakePe());
  assert.ok(base.equals(normalizePe(fakePe({ timestamp: 0x60000000 }))), "COFF TimeDateStamp");
  assert.ok(base.equals(normalizePe(fakePe({ checkSum: 0xbeef }))), "optional header CheckSum");
  assert.ok(base.equals(normalizePe(fakePe({ debugStamp: 0x12345678 }))), "debug directory TimeDateStamp");
  assert.ok(base.equals(normalizePe(fakePe({ guid: 0x77 }))), "RSDS GUID");
  assert.ok(base.equals(normalizePe(fakePe({ age: 9 }))), "RSDS age");
  assert.ok(base.equals(normalizePe(fakePe({ repro: 0x55 }))), "REPRO data");
  assert.ok(base.equals(normalizePe(fakePe({ certificate: winCertificate() }))), "an appended certificate table");
  assert.ok(
    normalizePe(fakePe({ certificate: winCertificate() })).equals(
      normalizePe(fakePe({ certificate: winCertificate(Buffer.from("another signer"), { type: 0x0002 }), timestamp: 1 })),
    ),
    "two signed copies",
  );
});

test("normalizePe keeps PEs unequal when their code differs", () => {
  const a = normalizePe(fakePe());
  const b = normalizePe(fakePe({ code: Buffer.alloc(32, 0xcc), timestamp: 0x60000000, certificate: winCertificate() }));
  assert.ok(!a.equals(b));
});

test("normalizePe returns a copy and leaves its argument alone", () => {
  const pe = fakePe({ timestamp: 0x61000000 });
  const before = Buffer.from(pe);
  const normalized = normalizePe(pe);
  assert.notEqual(normalized, pe);
  assert.ok(pe.equals(before));
  assert.equal(normalized.readUInt32LE(0x48), 0);
});

test("normalizePe on a truncated or non-PE buffer does not throw", () => {
  const pe = fakePe({ certificate: winCertificate() });
  for (let n = 0; n <= pe.length; n += 1) {
    const cut = pe.subarray(0, n);
    assert.ok(normalizePe(cut).length <= n, `length ${n}`);
  }
  assert.equal(normalizePe(Buffer.from("MZ")).toString("latin1"), "MZ");
  assert.doesNotThrow(() => normalizePe(Buffer.alloc(0x3c, 0xff)));
  const junk = Buffer.from(pe);
  junk.fill(0xff, 0x40, 0x100);
  assert.doesNotThrow(() => normalizePe(junk));
  const plain = Buffer.from("plain text, not a PE file");
  assert.ok(normalizePe(plain).equals(plain));
});

test("versionResourceRange finds the VERSIONINFO data, and null without one", () => {
  const pe = fakePe({ strings: { ProductName: "Clauseo" } });
  const range = versionResourceRange(pe);
  assert.ok(range);
  assert.equal(range.start, RAW + 88);
  assert.ok(range.end > range.start && range.end <= pe.length);
  assert.equal(pe.toString("utf16le", range.start + 6, range.start + 38).split("\0")[0], "VS_VERSION_INFO");
  assert.equal(versionResourceRange(fakePe({ strings: null })), null);
  assert.equal(versionResourceRange(Buffer.from("not a PE file")), null);
  assert.equal(versionResourceRange(pe.subarray(0, RAW + 100)), null);
});

test("compareTrees: an identical file, nested files, and files only on one side", () => {
  const root = tempDir();
  try {
    const a = writeTree(join(root, "a"), { "readme.txt": "same", "plugins/helper.dll": "helper", "only-a.txt": "a" });
    const b = writeTree(join(root, "b"), { "readme.txt": "same", "plugins/helper.dll": "helper", "only-b.txt": "b" });
    const result = compareTrees(a, b);
    assert.deepEqual(result.identical, ["plugins/helper.dll", "readme.txt"]);
    assert.deepEqual(result.onlyA, ["only-a.txt"]);
    assert.deepEqual(result.onlyB, ["only-b.txt"]);
    assert.deepEqual(result.expected, []);
    assert.deepEqual(result.unexpected, [
      { path: "only-a.txt", reason: "only in the baseline" },
      { path: "only-b.txt", reason: "only in the candidate" },
    ]);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("compareTrees skips symbolic links", { skip: process.platform === "win32" && "symlinks need rights on Windows" }, () => {
  const root = tempDir();
  try {
    const a = writeTree(join(root, "a"), { "readme.txt": "same" });
    const b = writeTree(join(root, "b"), { "readme.txt": "same" });
    symlinkSync(join(a, "readme.txt"), join(a, "link.txt"));
    const result = compareTrees(a, b);
    assert.deepEqual(result.identical, ["readme.txt"]);
    assert.deepEqual(result.onlyA, []);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("compareTrees: a signature and an uninstaller are expected, whatever their differences", () => {
  const root = tempDir();
  try {
    const a = writeTree(join(root, "a"), {
      "Clauseo.exe.sig": "sig-one",
      "uninstall.exe": fakePe({ timestamp: 1, debugStamp: 1 }),
      "uninstall-beyond.exe": fakePe({ timestamp: 1 }),
    });
    const b = writeTree(join(root, "b"), {
      "Clauseo.exe.sig": "sig-two",
      "uninstall.exe": fakePe({ timestamp: 2, debugStamp: 2, certificate: winCertificate() }),
      "uninstall-beyond.exe": fakePe({ timestamp: 2, code: Buffer.alloc(32, 0xcc) }),
    });
    const result = compareTrees(a, b);
    assert.deepEqual(result.unexpected, []);
    assert.deepEqual(result.expected, [
      {
        path: "Clauseo.exe.sig",
        reason: "an updater signature: each dry run signs with its own throwaway key",
      },
      {
        path: "uninstall-beyond.exe",
        reason: "the uninstaller, which makensis rebuilds in each run (it differs beyond timestamps and debug IDs)",
      },
      {
        path: "uninstall.exe",
        reason: "the uninstaller, which makensis rebuilds in each run (the same after timestamps and debug IDs are cleared)",
      },
    ]);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("compareTrees: a PE file that differs only in timestamps is expected; one that differs in code is not", () => {
  const root = tempDir();
  try {
    const a = writeTree(join(root, "a"), { "app.exe": fakePe({ timestamp: 1 }), "core.dll": fakePe({ code: Buffer.alloc(32, 0x90) }) });
    const b = writeTree(join(root, "b"), {
      "app.exe": fakePe({ timestamp: 2, debugStamp: 3, checkSum: 7 }),
      "core.dll": fakePe({ code: Buffer.alloc(32, 0xcc), timestamp: 2 }),
    });
    const result = compareTrees(a, b);
    assert.deepEqual(result.expected, [{ path: "app.exe", reason: "timestamps and debug IDs only" }]);
    assert.equal(result.unexpected.length, 1);
    const [{ path, reason }] = result.unexpected;
    assert.equal(path, "core.dll");
    const offset = firstByteDifference(fakePe({ code: Buffer.alloc(32, 0x90) }), fakePe({ code: Buffer.alloc(32, 0xcc) }));
    assert.equal(
      reason,
      `a PE file that differs beyond timestamps, debug IDs and its version resource (first difference at offset 0x${offset.toString(16)})`,
    );
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("compareTrees: a difference only in the version resource is expected", () => {
  const root = tempDir();
  try {
    const a = writeTree(join(root, "a"), { "app.exe": fakePe({ strings: { FileDescription: "aaaa" } }) });
    const b = writeTree(join(root, "b"), { "app.exe": fakePe({ strings: { FileDescription: "bbbb" }, timestamp: 9 }) });
    const result = compareTrees(a, b);
    assert.deepEqual(result.expected, [{ path: "app.exe", reason: "the version resource only" }]);
    assert.deepEqual(result.unexpected, []);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("compareTrees: a version resource of another length, or a difference beyond it, is unexpected", () => {
  const root = tempDir();
  try {
    const a = writeTree(join(root, "a"), { "app.exe": fakePe({ strings: { FileDescription: "aaaa" } }) });
    const b = writeTree(join(root, "b"), { "app.exe": fakePe({ strings: { FileDescription: "bbbbbbbb" } }) });
    const result = compareTrees(a, b);
    assert.deepEqual(result.expected, []);
    assert.match(result.unexpected[0].reason, /^a PE file that differs beyond timestamps, debug IDs and its version resource/);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("compareTrees: a decompiled NSIS script that differs is unexpected, with a diff", () => {
  const root = tempDir();
  try {
    const a = writeTree(join(root, "a"), { "[NSIS].nsi": "Name Clauseo\nOutFile setup.exe\nSection Main\n" });
    const b = writeTree(join(root, "b"), { "[NSIS].nsi": "Name Clauseo\nOutFile setup2.exe\nSection Main\n" });
    const result = compareTrees(a, b);
    assert.equal(result.unexpected.length, 1);
    const [{ path, reason, diff }] = result.unexpected;
    assert.equal(path, "[NSIS].nsi");
    assert.equal(reason, "the decompiled NSIS script differs");
    assert.match(diff, /^-OutFile setup\.exe$/m);
    assert.match(diff, /^\+OutFile setup2\.exe$/m);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("compareTrees: the NSIS diff falls back to the first differing lines when diff cannot run", () => {
  const root = tempDir();
  const empty = tempDir();
  const saved = process.env.PATH;
  try {
    const a = writeTree(join(root, "a"), { "[NSIS].nsi": "one\ntwo\nthree\n" });
    const b = writeTree(join(root, "b"), { "[NSIS].nsi": "one\n2\nthree\n" });
    process.env.PATH = empty;
    const result = compareTrees(a, b);
    assert.equal(result.unexpected[0].diff, "@@ line 2 @@\n-two\n+2");
  } finally {
    process.env.PATH = saved;
    rmSync(root, { recursive: true, force: true });
    rmSync(empty, { recursive: true, force: true });
  }
});

test("compareTrees: any other file that differs is unexpected", () => {
  const root = tempDir();
  try {
    const a = writeTree(join(root, "a"), { "notes.txt": "one", "fake.exe": "MZ not a PE" });
    const b = writeTree(join(root, "b"), { "notes.txt": "two", "fake.exe": "MZ still not a PE" });
    const result = compareTrees(a, b);
    assert.deepEqual(result.unexpected, [
      { path: "fake.exe", reason: "differs" },
      { path: "notes.txt", reason: "differs" },
    ]);
    assert.deepEqual(result.expected, []);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("the CLI exits 2 on wrong arguments, a missing folder, or folders with no installer", () => {
  const root = tempDir();
  try {
    const empty = join(root, "empty");
    mkdirSync(empty);
    const run = (args) => spawnSync(process.execPath, [SCRIPT, ...args], { encoding: "utf8" });
    assert.equal(run([]).status, 2);
    assert.equal(run([empty]).status, 2);
    assert.equal(run([empty, empty, empty]).status, 2);
    assert.match(run([empty]).stderr, /usage: node .github\/scripts\/compare-installers\.mjs/);
    assert.equal(run([join(root, "missing"), empty]).status, 2);
    assert.equal(run([empty, empty]).status, 2);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

/**
 * Sets up two dist folders and a stand-in 7z. The stand-in copies the folder named on the first line
 * of the installer file ("CONTENTS <folder>") into the -o folder; it is first on PATH for the CLI.
 */
function distFixture(root) {
  const bin = join(root, "bin");
  mkdirSync(bin);
  const sevenZip = join(bin, "7z");
  writeFileSync(
    sevenZip,
    [
      "#!/bin/sh",
      'for arg in "$@"; do',
      '  case "$arg" in -o*) out="${arg#-o}" ;; esac',
      '  last="$arg"',
      "done",
      "src=$(sed -n 's/^CONTENTS //p' \"$last\")",
      'cp -R "$src/." "$out/" || exit 2',
      "",
    ].join("\n"),
  );
  chmodSync(sevenZip, 0o755);
  const contentsA = writeTree(join(root, "contents-a"), {
    "app.exe": fakePe({ timestamp: 1 }),
    "uninstall.exe": fakePe({ timestamp: 1 }),
    "[NSIS].nsi": "Name Clauseo\n",
    "readme.txt": "same",
  });
  const contentsB = writeTree(join(root, "contents-b"), {
    "app.exe": fakePe({ timestamp: 2 }),
    "uninstall.exe": fakePe({ timestamp: 3, debugStamp: 4 }),
    "[NSIS].nsi": "Name Clauseo\n",
    "readme.txt": "same",
  });
  const installer = (name, contents) => [name, `CONTENTS ${contents}\n`];
  const baseline = writeTree(join(root, "baseline"), {
    ...Object.fromEntries([
      installer("Clauseo_1.0.0_x64-setup.exe", contentsA),
      installer("Clauseo_1.0.0_x64-machine-setup.exe", contentsA),
    ]),
    "latest.json": "{}",
  });
  const candidate = writeTree(join(root, "candidate"), Object.fromEntries([
    installer("Clauseo_1.0.1_x64-setup.exe", contentsB),
    installer("Clauseo_1.0.1_x64-machine-setup.exe", contentsB),
  ]));
  return { bin, baseline, candidate, contentsB };
}

test("the CLI compares paired installers: information, expected differences, exit 0", { skip: process.platform === "win32" && "the stand-in 7z is a shell script" }, () => {
  const root = tempDir();
  try {
    const { bin, baseline, candidate } = distFixture(root);
    const result = spawnSync(process.execPath, [SCRIPT, baseline, candidate], {
      encoding: "utf8",
      env: { ...process.env, PATH: `${bin}${delimiter}${process.env.PATH}` },
    });
    assert.equal(result.status, 0, result.stderr);
    assert.match(result.stdout, /^information: latest\.json is only in the baseline dist$/m);
    assert.match(result.stdout, /^== Clauseo_1\.0\.0_x64-machine-setup\.exe vs Clauseo_1\.0\.1_x64-machine-setup\.exe ==$/m);
    assert.match(result.stdout, /^== Clauseo_1\.0\.0_x64-setup\.exe vs Clauseo_1\.0\.1_x64-setup\.exe ==$/m);
    assert.match(result.stdout, /^unexpected \(0\):$/m);
    assert.match(result.stdout, /^  app\.exe: timestamps and debug IDs only$/m);
    assert.match(result.stdout, /^  uninstall\.exe: the uninstaller, which makensis rebuilds in each run \(the same after timestamps and debug IDs are cleared\)$/m);
    assert.match(result.stdout, /^identical: 2 files$/m);
    assert.match(result.stdout, /^verdict: the installers hold the same files, apart from build-to-build differences \(2 pairs\)$/m);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("the CLI exits 1 on an unexpected difference and on an installer with no partner", { skip: process.platform === "win32" && "the stand-in 7z is a shell script" }, () => {
  const root = tempDir();
  try {
    const { bin, baseline, candidate, contentsB } = distFixture(root);
    writeFileSync(join(contentsB, "readme.txt"), "changed");
    writeFileSync(join(candidate, "Clauseo_1.0.1_arm64-setup.exe"), `CONTENTS ${contentsB}\n`);
    const result = spawnSync(process.execPath, [SCRIPT, baseline, candidate], {
      encoding: "utf8",
      env: { ...process.env, PATH: `${bin}${delimiter}${process.env.PATH}` },
    });
    assert.equal(result.status, 1, result.stderr);
    assert.match(result.stdout, /^  readme\.txt: differs$/m);
    assert.match(result.stdout, /^  Clauseo_1\.0\.1_arm64-setup\.exe: no partner in the baseline dist$/m);
    assert.match(result.stdout, /^verdict: 3 unexpected difference\(s\)/m);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("the CLI exits 2 when 7-Zip is not on PATH", () => {
  const root = tempDir();
  const empty = join(root, "no-tools");
  mkdirSync(empty);
  try {
    const { baseline, candidate } = distFixture(root);
    const result = spawnSync(process.execPath, [SCRIPT, baseline, candidate], {
      encoding: "utf8",
      env: { PATH: empty },
    });
    assert.equal(result.status, 2);
    assert.match(result.stderr, /7z is not installed or not on PATH/);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
