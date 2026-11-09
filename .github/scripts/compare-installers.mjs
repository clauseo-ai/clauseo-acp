// Compares the Windows installers of two release dry runs of the same version, one on main and one
// on this branch, and says which differences are build-to-build noise and which are real. Each run's
// installers are in its `dist-assets` artifact; download each into a folder of its own:
//
//   gh run download <baseline-run-id> -n dist-assets -D <baseline-dir>     (the run on main)
//   gh run download <candidate-run-id> -n dist-assets -D <candidate-dir>   (the run on this branch)
//   node .github/scripts/compare-installers.mjs <baseline-dir> <candidate-dir>
//
// The installers are the top-level files Clauseo_<version>_<arch>-setup.exe and
// Clauseo_<version>_<arch>-machine-setup.exe. They pair by arch and machine-wide or not, whatever the
// version. Each pair is unpacked with 7-Zip (`7z` on PATH) into a temporary folder, and the two trees
// are compared file by file. Expected, build to build: an updater .sig (each run signs with its own
// throwaway key); the uninstaller (makensis rebuilds it in each run); a PE file that differs only in
// timestamps, debug IDs, its version resource or its certificate table. Anything else is unexpected:
// a file only one side has, a PE file that differs in code, the decompiled [NSIS].nsi script, another
// file that differs, and an installer with no partner. Asset names present in only one folder are
// printed as information, since a tier 2 leg may be missing.
//
// Exit status: 0 when nothing is unexpected, 1 when something is, 2 on a usage or tool error (7-Zip
// missing or failing, or no installer in either folder). Plain Node: no package to install.

import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdirSync, mkdtempSync, readFileSync, readdirSync, realpathSync, rmSync, statSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const INSTALLER = /^Clauseo_(\d+\.\d+\.\d+)_(x64|arm64)-(machine-)?setup\.exe$/;
const USAGE = "usage: node .github/scripts/compare-installers.mjs <baseline-dist> <candidate-dist>\n";
const RT_VERSION = 16;
const DIFF_CONTEXT_LINES = 2;
const DIFF_MAX_LINES = 200;
const FALLBACK_DIFF_LINES = 40;
const DEBUG_ENTRY_SIZE = 28;

/** SHA-256 of <bytes>, as hex. */
function sha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

/** Sorts objects with a `path` field by it. */
function byPath(x, y) {
  return x.path < y.path ? -1 : x.path > y.path ? 1 : 0;
}

/**
 * Every regular file under <root> as a map from its path relative to <root> (with `/` separators) to
 * its full path. Symbolic links are skipped, not followed.
 */
function walk(root) {
  const files = new Map();
  const visit = (dir, prefix) => {
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      if (entry.isSymbolicLink()) {
        continue;
      }
      const relative = `${prefix}${entry.name}`;
      if (entry.isDirectory()) {
        visit(join(dir, entry.name), `${relative}/`);
      } else if (entry.isFile()) {
        files.set(relative, join(dir, entry.name));
      }
    }
  };
  visit(root, "");
  return files;
}

/** Throws a RangeError unless [offset, offset + length) lies inside a buffer of <size> bytes. */
function within(size, offset, length) {
  if (!Number.isSafeInteger(offset) || offset < 0 || offset + length > size) {
    throw new RangeError(`offset ${offset} is outside the file`);
  }
  return offset;
}

/** The little-endian u16 at <offset> of <buffer>; a RangeError when it is outside the buffer. */
function u16(buffer, offset) {
  return buffer.readUInt16LE(within(buffer.length, offset, 2));
}

/** The little-endian u32 at <offset> of <buffer>; a RangeError when it is outside the buffer. */
function u32(buffer, offset) {
  return buffer.readUInt32LE(within(buffer.length, offset, 4));
}

/** Zeroes <length> bytes of <buffer> at <offset>; a RangeError when they are outside the buffer. */
function zero(buffer, offset, length) {
  buffer.fill(0, within(buffer.length, offset, length), offset + length);
}

/** The offset of the PE signature of <buffer>; a RangeError when <buffer> is not a PE file. */
function peSignature(buffer) {
  if (buffer.toString("latin1", 0, 2) !== "MZ") {
    throw new RangeError("no MZ header");
  }
  const nt = u32(buffer, 0x3c);
  if (buffer.toString("latin1", within(buffer.length, nt, 4), nt + 4) !== "PE\0\0") {
    throw new RangeError("no PE signature");
  }
  return nt;
}

/** True when <buffer> is a PE file: MZ, and a PE signature at the offset its header names. */
function isPe(buffer) {
  try {
    peSignature(buffer);
    return true;
  } catch {
    return false;
  }
}

/**
 * The optional header of the PE file whose signature is at <nt>: where its data directories start,
 * how many it declares, and each one as { address, size } (null past the declared ones). A
 * RangeError when the magic is neither PE32 nor PE32+ or the header runs past the end of <buffer>.
 */
function dataDirectories(buffer, nt) {
  const optional = nt + 24;
  const magic = u16(buffer, optional);
  if (magic !== 0x10b && magic !== 0x20b) {
    throw new RangeError(`optional header magic 0x${magic.toString(16)}`);
  }
  const plus = magic === 0x20b;
  const count = u32(buffer, optional + (plus ? 108 : 92));
  const first = optional + (plus ? 112 : 96);
  return {
    optional,
    entryAt: (index) => first + index * 8,
    directory: (index) =>
      index < count
        ? { address: u32(buffer, first + index * 8), size: u32(buffer, first + index * 8 + 4) }
        : null,
  };
}

/**
 * Maps an RVA of the PE file whose signature is at <nt> to its file offset through the section table
 * that follows the optional header at <optional>. The returned function throws a RangeError when no
 * section holds the RVA.
 */
function sectionOffsets(buffer, nt, optional) {
  const sections = u16(buffer, nt + 6);
  const table = optional + u16(buffer, nt + 20);
  return (rva) => {
    for (let i = 0; i < sections; i += 1) {
      const at = table + i * 40;
      const address = u32(buffer, at + 12);
      const size = Math.max(u32(buffer, at + 8), u32(buffer, at + 16));
      if (rva >= address && rva < address + size) {
        return rva - address + u32(buffer, at + 20);
      }
    }
    throw new RangeError(`RVA 0x${rva.toString(16)} is in no section`);
  };
}

/**
 * A copy of PE file <buffer> with what differs between two builds of the same code zeroed: the COFF
 * TimeDateStamp, the optional header CheckSum, the export and resource directories' timestamps, each
 * debug directory entry's timestamp, a CodeView RSDS entry's GUID and age, a REPRO entry's data; and
 * the certificate table, its data-directory entry zeroed and everything from its file offset on
 * dropped, so a signed copy and an unsigned one compare equal. A structure that runs past the end of
 * the file stops the normalizing there, and the copy returned holds what was done so far. A buffer
 * that is not a PE file comes back unchanged.
 * @param {Buffer} buffer the file's bytes; not changed
 * @returns {Buffer} the normalized copy
 */
export function normalizePe(buffer) {
  const out = Buffer.from(buffer);
  let certificateAt = 0;
  try {
    const nt = peSignature(out);
    zero(out, nt + 8, 4);
    const dirs = dataDirectories(out, nt);
    zero(out, dirs.optional + 64, 4);
    const security = dirs.directory(4);
    if (security !== null) {
      zero(out, dirs.entryAt(4), 8);
      certificateAt = security.address;
    }
    const offsetOf = sectionOffsets(out, nt, dirs.optional);
    const exportTable = dirs.directory(0);
    if (exportTable !== null && exportTable.address !== 0) {
      zero(out, offsetOf(exportTable.address) + 4, 4);
    }
    const resources = dirs.directory(2);
    if (resources !== null && resources.address !== 0) {
      zero(out, offsetOf(resources.address) + 4, 4);
    }
    const debug = dirs.directory(6);
    if (debug !== null && debug.address !== 0) {
      const base = offsetOf(debug.address);
      for (let i = 0; i < Math.floor(debug.size / DEBUG_ENTRY_SIZE); i += 1) {
        const entry = base + i * DEBUG_ENTRY_SIZE;
        zero(out, entry + 4, 4);
        const type = u32(out, entry + 12);
        const size = u32(out, entry + 16);
        const at = u32(out, entry + 24);
        if (at === 0) {
          continue;
        }
        if (type === 2 && out.toString("latin1", within(out.length, at, 4), at + 4) === "RSDS") {
          zero(out, at + 4, 16);
          zero(out, at + 20, 4);
        } else if (type === 16) {
          zero(out, at, size);
        }
      }
    }
  } catch (error) {
    if (!(error instanceof RangeError)) {
      throw error;
    }
  }
  return certificateAt > 0 && certificateAt < out.length ? out.subarray(0, certificateAt) : out;
}

/**
 * The file range { start, end } of the data of the first RT_VERSION (type 16) resource of PE file
 * <buffer>, following the first entry at each level below the type down to its leaf. null when the
 * file has no such resource, or its resource tree does not parse.
 * @param {Buffer} buffer the file's bytes
 * @returns {{ start: number, end: number } | null}
 */
export function versionResourceRange(buffer) {
  try {
    const nt = peSignature(buffer);
    const dirs = dataDirectories(buffer, nt);
    const resources = dirs.directory(2);
    if (resources === null || resources.address === 0) {
      return null;
    }
    const offsetOf = sectionOffsets(buffer, nt, dirs.optional);
    const root = offsetOf(resources.address);
    let directory = null;
    const entries = u16(buffer, root + 12) + u16(buffer, root + 14);
    for (let i = 0; i < entries; i += 1) {
      const at = root + 16 + i * 8;
      const offset = u32(buffer, at + 4);
      if (u32(buffer, at) === RT_VERSION && offset >= 0x80000000) {
        directory = root + (offset - 0x80000000);
        break;
      }
    }
    for (let depth = 0; directory !== null && depth < 4; depth += 1) {
      if (u16(buffer, directory + 12) + u16(buffer, directory + 14) === 0) {
        return null;
      }
      const offset = u32(buffer, directory + 20);
      if (offset >= 0x80000000) {
        directory = root + (offset - 0x80000000);
        continue;
      }
      const data = root + offset;
      const start = offsetOf(u32(buffer, data));
      const end = start + u32(buffer, data + 4);
      return end <= buffer.length ? { start, end } : null;
    }
    return null;
  } catch (error) {
    if (error instanceof RangeError) {
      return null;
    }
    throw error;
  }
}

/** The index of the first byte where <x> and <y> differ (a missing byte counts), or -1 when none does. */
function firstDifference(x, y) {
  const length = Math.max(x.length, y.length);
  for (let i = 0; i < length; i += 1) {
    if (x[i] !== y[i]) {
      return i;
    }
  }
  return -1;
}

/**
 * True when every byte where the normalized copies <normalizedA> and <normalizedB> differ lies inside
 * the version resource of both files, <a> and <b> (the same offsets in each).
 */
function onlyVersionResourceDiffers(a, b, normalizedA, normalizedB) {
  const rangeA = versionResourceRange(a);
  const rangeB = versionResourceRange(b);
  if (rangeA === null || rangeB === null) {
    return false;
  }
  const inside = (range, i) => i >= range.start && i < range.end;
  const length = Math.max(normalizedA.length, normalizedB.length);
  for (let i = 0; i < length; i += 1) {
    if (normalizedA[i] === normalizedB[i]) {
      continue;
    }
    if (i >= normalizedA.length || i >= normalizedB.length || !inside(rangeA, i) || !inside(rangeB, i)) {
      return false;
    }
  }
  return true;
}

/**
 * A short line diff of two text files: `diff -U 2` when it runs, else the first 40 differing lines of
 * each, as `-` (baseline) and `+` (candidate) lines. The diff is cut at 200 lines.
 */
function lineDiff(relative, pathA, pathB, a, b) {
  const result = spawnSync(
    "diff",
    [
      "-U",
      String(DIFF_CONTEXT_LINES),
      "--label",
      `baseline/${relative}`,
      "--label",
      `candidate/${relative}`,
      pathA,
      pathB,
    ],
    { encoding: "utf8", maxBuffer: 64 * 1024 * 1024 },
  );
  if (!result.error && result.status === 1) {
    const lines = result.stdout.replace(/\n$/, "").split("\n");
    if (lines.length <= DIFF_MAX_LINES) {
      return lines.join("\n");
    }
    return [...lines.slice(0, DIFF_MAX_LINES), `... ${lines.length - DIFF_MAX_LINES} more lines`].join("\n");
  }
  const linesA = a.toString("utf8").split("\n");
  const linesB = b.toString("utf8").split("\n");
  const out = [];
  let differing = 0;
  for (let i = 0; i < Math.max(linesA.length, linesB.length) && differing < FALLBACK_DIFF_LINES; i += 1) {
    if (linesA[i] === linesB[i]) {
      continue;
    }
    differing += 1;
    out.push(`@@ line ${i + 1} @@`);
    if (i < linesA.length) {
      out.push(`-${linesA[i]}`);
    }
    if (i < linesB.length) {
      out.push(`+${linesB[i]}`);
    }
  }
  return out.join("\n");
}

/**
 * Says whether the bytes of one file that differs between the trees is an expected build-to-build
 * difference: { expected, reason, diff } with diff only for the NSIS script. <relative> is the file's
 * path in the trees, <pathA> and <pathB> its full paths and <a> and <b> its bytes.
 */
function classify(relative, pathA, pathB, a, b) {
  const name = relative.slice(relative.lastIndexOf("/") + 1);
  if (name.endsWith(".sig")) {
    return { expected: true, reason: "an updater signature: each dry run signs with its own throwaway key" };
  }
  if (isPe(a) && isPe(b)) {
    const normalizedA = normalizePe(a);
    const normalizedB = normalizePe(b);
    if (/uninst/i.test(name)) {
      const note = normalizedA.equals(normalizedB)
        ? "the same after timestamps and debug IDs are cleared"
        : "it differs beyond timestamps and debug IDs";
      return { expected: true, reason: `the uninstaller, which makensis rebuilds in each run (${note})` };
    }
    if (normalizedA.equals(normalizedB)) {
      return { expected: true, reason: "timestamps and debug IDs only" };
    }
    if (onlyVersionResourceDiffers(a, b, normalizedA, normalizedB)) {
      return { expected: true, reason: "the version resource only" };
    }
    const at = firstDifference(normalizedA, normalizedB);
    return {
      expected: false,
      reason: `a PE file that differs beyond timestamps, debug IDs and its version resource (first difference at offset 0x${at.toString(16)})`,
    };
  }
  if (name === "[NSIS].nsi") {
    return {
      expected: false,
      reason: "the decompiled NSIS script differs",
      diff: lineDiff(relative, pathA, pathB, a, b),
    };
  }
  return { expected: false, reason: "differs" };
}

/**
 * Compares two folder trees file by file. Files are keyed by their path relative to each root, with
 * `/` separators; symbolic links are skipped. Each path is identical (same SHA-256), only in one tree,
 * or differs; a difference is classified by classify (expected build-to-build, or unexpected). Files
 * only in one tree are unexpected too, with the reasons "only in the baseline" and "only in the
 * candidate". Every list is sorted by path.
 * @param {string} dirA the baseline tree
 * @param {string} dirB the candidate tree
 * @returns {{ identical: string[], onlyA: string[], onlyB: string[], expected: {path: string, reason: string}[], unexpected: {path: string, reason: string, diff?: string}[] }}
 */
export function compareTrees(dirA, dirB) {
  const filesA = walk(dirA);
  const filesB = walk(dirB);
  const result = { identical: [], onlyA: [], onlyB: [], expected: [], unexpected: [] };
  for (const path of [...filesA.keys()].sort()) {
    if (!filesB.has(path)) {
      result.onlyA.push(path);
      result.unexpected.push({ path, reason: "only in the baseline" });
      continue;
    }
    const pathA = filesA.get(path);
    const pathB = filesB.get(path);
    const a = readFileSync(pathA);
    const b = readFileSync(pathB);
    if (sha256(a) === sha256(b)) {
      result.identical.push(path);
      continue;
    }
    const { expected, reason, diff } = classify(path, pathA, pathB, a, b);
    if (expected) {
      result.expected.push({ path, reason });
    } else {
      result.unexpected.push(diff === undefined ? { path, reason } : { path, reason, diff });
    }
  }
  for (const path of [...filesB.keys()].sort()) {
    if (!filesA.has(path)) {
      result.onlyB.push(path);
      result.unexpected.push({ path, reason: "only in the candidate" });
    }
  }
  result.expected.sort(byPath);
  result.unexpected.sort(byPath);
  return result;
}

/**
 * The installers in folder <dir> (top level only), keyed by arch and machine-wide or not, and the
 * names of its other files. Throws when <dir> holds two installers with the same key.
 */
function inventory(dir) {
  const installers = new Map();
  const assets = new Set();
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    if (!entry.isFile()) {
      continue;
    }
    const match = INSTALLER.exec(entry.name);
    if (match === null) {
      assets.add(entry.name);
      continue;
    }
    const key = `${match[2]}${match[3] ? "-machine" : ""}`;
    if (installers.has(key)) {
      throw new Error(`${dir} holds two ${key} installers: ${installers.get(key).name} and ${entry.name}`);
    }
    installers.set(key, { name: entry.name, path: join(dir, entry.name) });
  }
  return { installers, assets };
}

/** Unpacks <installer> into <dest> with 7-Zip. Exit codes 0 and 1 (a warning) count as success. */
function extract(installer, dest) {
  const result = spawnSync("7z", ["x", "-y", "-aou", `-o${dest}`, installer], {
    stdio: ["ignore", "ignore", "pipe"],
    encoding: "utf8",
  });
  if (result.error?.code === "ENOENT") {
    throw new Error("7z is not installed or not on PATH: install 7-Zip to compare the installers");
  }
  if (result.error) {
    throw new Error(`7z could not run on ${installer}: ${result.error.message}`);
  }
  if (result.status !== 0 && result.status !== 1) {
    const detail = (result.stderr ?? "").trim().split("\n").slice(-3).join(" | ");
    const how = result.status === null ? `signal ${result.signal}` : `exit ${result.status}`;
    throw new Error(`7z failed on ${installer} (${how})${detail === "" ? "" : `: ${detail}`}`);
  }
}

/**
 * Unpacks both installers into temporary folders, compares the two trees and removes the folders.
 * Throws on a 7-Zip failure.
 */
function compareInstallers(baselinePath, candidatePath) {
  const work = mkdtempSync(join(tmpdir(), "compare-installers-"));
  try {
    const baselineTree = join(work, "baseline");
    const candidateTree = join(work, "candidate");
    mkdirSync(baselineTree);
    mkdirSync(candidateTree);
    extract(baselinePath, baselineTree);
    extract(candidatePath, candidateTree);
    return compareTrees(baselineTree, candidateTree);
  } finally {
    rmSync(work, { recursive: true, force: true });
  }
}

/** The report lines for one comparison: its unexpected and expected differences, and the identical count. */
function reportLines(title, result) {
  const lines = [`== ${title} ==`, `unexpected (${result.unexpected.length}):`];
  for (const { path, reason, diff } of result.unexpected) {
    lines.push(`  ${path}: ${reason}`);
    if (diff !== undefined) {
      lines.push(...diff.split("\n").map((line) => `    ${line}`));
    }
  }
  lines.push(`expected build-to-build (${result.expected.length}):`);
  for (const { path, reason } of result.expected) {
    lines.push(`  ${path}: ${reason}`);
  }
  lines.push(`identical: ${result.identical.length} files`);
  return lines;
}

/** The result for an installer with no partner in the other folder: one unexpected difference. */
function unpairedResult(baseline, candidate) {
  const result = { identical: [], onlyA: [], onlyB: [], expected: [], unexpected: [] };
  if (baseline !== undefined) {
    result.unexpected.push({ path: baseline.name, reason: "no partner in the candidate dist" });
  } else {
    result.unexpected.push({ path: candidate.name, reason: "no partner in the baseline dist" });
  }
  return result;
}

/** The CLI: prints the comparison and returns the exit status (0, 1 or 2). */
function main(argv) {
  if (argv.length !== 2) {
    process.stderr.write(USAGE);
    return 2;
  }
  const [baselineDir, candidateDir] = argv;
  try {
    for (const dir of [baselineDir, candidateDir]) {
      if (!statSync(dir, { throwIfNoEntry: false })?.isDirectory()) {
        throw new Error(`${dir} is not a folder`);
      }
    }
    const baseline = inventory(baselineDir);
    const candidate = inventory(candidateDir);
    const keys = [...new Set([...baseline.installers.keys(), ...candidate.installers.keys()])].sort();
    if (keys.length === 0) {
      throw new Error(`no Windows installers in ${baselineDir} or ${candidateDir}`);
    }
    const lines = [];
    for (const name of [...baseline.assets].filter((n) => !candidate.assets.has(n)).sort()) {
      lines.push(`information: ${name} is only in the baseline dist`);
    }
    for (const name of [...candidate.assets].filter((n) => !baseline.assets.has(n)).sort()) {
      lines.push(`information: ${name} is only in the candidate dist`);
    }
    let unexpected = 0;
    let pairs = 0;
    for (const key of keys) {
      const a = baseline.installers.get(key);
      const b = candidate.installers.get(key);
      const title = `${a?.name ?? "(no baseline installer)"} vs ${b?.name ?? "(no candidate installer)"}`;
      let result;
      if (a !== undefined && b !== undefined) {
        pairs += 1;
        result = compareInstallers(a.path, b.path);
      } else {
        result = unpairedResult(a, b);
      }
      unexpected += result.unexpected.length;
      lines.push(...reportLines(title, result));
    }
    lines.push(
      unexpected === 0
        ? `verdict: the installers hold the same files, apart from build-to-build differences (${pairs} pairs)`
        : `verdict: ${unexpected} unexpected difference(s); the installers do not hold the same files`,
    );
    process.stdout.write(`${lines.join("\n")}\n`);
    return unexpected === 0 ? 0 : 1;
  } catch (error) {
    process.stderr.write(`compare-installers: ${error.message}\n`);
    return 2;
  }
}

// Real paths on both sides: run through a symbolic link, the script still runs (a comparison that
// silently did nothing would pass).
// argv[1] is not a path when another script imports this module (`node -e` passes its own
// arguments there), so a path that does not resolve means "not run as a script".
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
