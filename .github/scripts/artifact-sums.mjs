// The SHA-256 of every file in an artifact, carried from the job that uploads it to the job that
// downloads it as a job output, so the downloading job uses exactly the bytes the uploading job
// made: release.yml's windows-exe-* (build to sign-windows) and release-* (build and sign-windows
// to assemble) artifacts, every file of each, the plug-ins and the machine installer included.
//
//   node artifact-sums.mjs sums <root> <file-or-folder>...
//     prints one line of JSON, {"<path>":"<sha256>",...}: every file given, folders walked
//     (symbolic links left out), each path relative to <root> with `/` separators, sorted.
//   node artifact-sums.mjs check <outputs-json> <key> <root> <file-or-folder>...
//     <outputs-json> is a job's outputs as toJSON gives them; its <key> holds what `sums` printed.
//     The files given, read the same way, must be exactly those files with those digests.
//
// Exit status: 0 when the sums are printed or match, 1 when they don't (each difference on
// stderr) or the key holds no sums, 2 on a usage error or a path that does not exist.

import { createHash } from "node:crypto";
import { existsSync, lstatSync, readdirSync, readFileSync, realpathSync } from "node:fs";
import { join, relative, resolve, sep } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

/**
 * Every file among <paths> (files, and folders walked, symbolic links left out) as
 * { "<path relative to root, / separated>": "<sha256 hex>" }, keys sorted. Throws when a path
 * does not exist or a file lies outside <root>.
 */
export function artifactSums(root, paths) {
  const sums = {};
  const visit = (path) => {
    const stat = lstatSync(path);
    if (stat.isDirectory()) {
      for (const name of readdirSync(path)) {
        visit(join(path, name));
      }
    } else if (stat.isFile()) {
      const name = relative(resolve(root), resolve(path)).split(sep).join("/");
      if (name === "" || name.startsWith("../") || name === "..") {
        throw new Error(`${path} is not inside ${root}`);
      }
      sums[name] = createHash("sha256").update(readFileSync(path)).digest("hex");
    }
  };
  for (const path of paths) {
    if (!existsSync(path)) {
      throw new Error(`no such file or folder: ${path}`);
    }
    visit(path);
  }
  return Object.fromEntries(Object.entries(sums).sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0)));
}

/**
 * The differences between the sums an uploading job recorded, <expected>, and the files found,
 * <found> (both as artifactSums returns them): one line each, empty when they are the same files.
 */
export function sumsProblems(expected, found) {
  const problems = [];
  for (const [name, digest] of Object.entries(expected)) {
    if (!(name in found)) {
      problems.push(`${name}: uploaded, but not downloaded`);
    } else if (found[name] !== digest) {
      problems.push(`${name}: SHA-256 ${found[name]}, but the uploading job recorded ${digest}`);
    }
  }
  for (const name of Object.keys(found)) {
    if (!(name in expected)) {
      problems.push(`${name}: downloaded, but the uploading job recorded no such file`);
    }
  }
  return problems;
}

/** The sums under <key> in <outputsJson> (toJSON of a job's outputs); throws when there are none. */
export function sumsFromOutputs(outputsJson, key) {
  const outputs = JSON.parse(outputsJson || "{}");
  const value = outputs?.[key];
  if (typeof value !== "string" || value.trim() === "") {
    throw new Error(`the uploading job set no output ${key}`);
  }
  const sums = JSON.parse(value);
  if (sums === null || typeof sums !== "object" || Array.isArray(sums) || Object.keys(sums).length === 0) {
    throw new Error(`the output ${key} lists no files`);
  }
  for (const [name, digest] of Object.entries(sums)) {
    if (!/^[0-9a-f]{64}$/.test(digest)) {
      throw new Error(`the output ${key} gives ${name} the digest ${JSON.stringify(digest)}`);
    }
  }
  return sums;
}

function main(argv) {
  const usage = () => {
    process.stderr.write(
      "usage: node artifact-sums.mjs sums <root> <file-or-folder>...\n" +
        "       node artifact-sums.mjs check <outputs-json> <key> <root> <file-or-folder>...\n",
    );
    return 2;
  };
  if (argv[0] === "sums" && argv.length >= 3) {
    let sums;
    try {
      sums = artifactSums(argv[1], argv.slice(2));
    } catch (error) {
      process.stderr.write(`sums: ${error.message}\n`);
      return 2;
    }
    if (Object.keys(sums).length === 0) {
      process.stderr.write("sums: no files\n");
      return 1;
    }
    process.stdout.write(`${JSON.stringify(sums)}\n`);
    return 0;
  }
  if (argv[0] === "check" && argv.length >= 5) {
    const [, outputsJson, key, root, ...paths] = argv;
    let expected;
    let found;
    try {
      expected = sumsFromOutputs(outputsJson, key);
    } catch (error) {
      process.stderr.write(`check ${key}: ${error.message}\n`);
      return 1;
    }
    try {
      found = artifactSums(root, paths);
    } catch (error) {
      process.stderr.write(`check ${key}: ${error.message}\n`);
      return 2;
    }
    const problems = sumsProblems(expected, found);
    for (const problem of problems) {
      process.stderr.write(`check ${key}: ${problem}\n`);
    }
    if (problems.length === 0) {
      process.stdout.write(`${key}: ${Object.keys(found).length} files, each as uploaded\n`);
    }
    return problems.length === 0 ? 0 : 1;
  }
  return usage();
}

// Run as a script, not when imported (the same guard as latest-json.mjs).
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
