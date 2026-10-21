// The planted test of the scripts' doors in ui/app/.oxlintrc.json: the pinned oxlint (the app's
// devDependency, run through buildtime-proc.mjs like any other tool) lints a scratch copy of
// scripts/ that holds one way around the bans per file, and must report each one, while the door
// files' own imports pass. The scratch tree lives under the OS temp dir: inside ui/app, the
// `oxlint` step of `cargo xtask check` would lint the planted files too.

import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { resolveBin } from "./buildtime-lib.mjs";
import { runTool } from "./buildtime-proc.mjs";

const APP_DIR = resolve(dirname(fileURLToPath(import.meta.url)), "..");
/** How long one oxlint run over the scratch tree may take; it takes well under a second. */
const OXLINT_TIMEOUT_MS = 60_000;

const IMPORTS = "eslint(no-restricted-imports)";
const PROPERTIES = "eslint(no-restricted-properties)";
const WRITE_DOOR = "files change only through scripts/guard.mjs";
const PROCESS_DOOR = "child processes start only through scripts/buildtime-proc.mjs";
const LOADER = "a module that loads or runs other code";
const OTHER_WRITER = "these modules write files past the node:fs ban";

/** Each planted form, as the file under scripts/ that holds it, and what oxlint must say. */
const BANNED = {
  "write-named.mjs": [`import { writeFileSync } from "node:fs";`, IMPORTS, WRITE_DOOR],
  "write-renamed.mjs": [`import { rm as remove } from "fs";`, IMPORTS, WRITE_DOOR],
  "write-promises.mjs": [`import { promises } from "node:fs";`, IMPORTS, WRITE_DOOR],
  "default.mjs": [`import fs from "node:fs";`, IMPORTS, WRITE_DOOR],
  "default-named.mjs": [`import { default as fs } from "fs";`, IMPORTS, WRITE_DOOR],
  "namespace.mjs": [`import * as fs from "node:fs";`, IMPORTS, WRITE_DOOR],
  "fs-promises.mjs": [`import { rm } from "fs/promises";`, IMPORTS, WRITE_DOOR],
  "node-fs-promises.mjs": [`import { readFile } from "node:fs/promises";`, IMPORTS, WRITE_DOOR],
  "reexport.mjs": [`export { writeFileSync } from "node:fs";`, IMPORTS, WRITE_DOOR],
  "reexport-star.mjs": [`export * from "fs";`, IMPORTS, WRITE_DOOR],
  "spawn.mjs": [`import { spawn } from "node:child_process";`, IMPORTS, PROCESS_DOOR],
  "exec.mjs": [`import { execSync } from "child_process";`, IMPORTS, PROCESS_DOOR],
  "process-dynamic.mjs": [`await import("node:child_process");`, IMPORTS, PROCESS_DOOR],
  "require.mjs": [`import { createRequire } from "node:module";`, IMPORTS, LOADER],
  "module.mjs": [`import module from "module";`, IMPORTS, LOADER],
  "worker.mjs": [`import { Worker } from "node:worker_threads";`, IMPORTS, LOADER],
  "vm.mjs": [`import { runInThisContext } from "vm";`, IMPORTS, LOADER],
  "builtin.mjs": [`process.getBuiltinModule("node:fs");`, PROPERTIES, "process.getBuiltinModule"],
  "builtin-global.mjs": [
    `globalThis.process["getBuiltinModule"]("fs");`,
    PROPERTIES,
    "process.getBuiltinModule",
  ],
  "builtin-destructured.mjs": [
    `const { getBuiltinModule } = process;`,
    PROPERTIES,
    "process.getBuiltinModule",
  ],
  "binding.mjs": [`process.binding("fs");`, PROPERTIES, "process.binding"],
  "eval.mjs": [`eval("1");`, "eslint(no-eval)", ""],
  "function.mjs": [`new Function("return 1");`, "eslint(no-new-func)", ""],
  "disable.mjs": [
    `// oxlint-disable-next-line no-restricted-imports\nimport { writeFileSync } from "node:fs";`,
    "eslint(no-warning-comments)",
    "",
  ],
  // Each door keeps the bans of the other doors.
  "guard.mjs": [`import { spawn } from "node:child_process";`, IMPORTS, PROCESS_DOOR],
  "buildtime-proc.mjs": [`import { writeFileSync } from "node:fs";`, IMPORTS, WRITE_DOOR],
  "buildtime-leader.mjs": [`import { createRequire } from "node:module";`, IMPORTS, LOADER],
  "guard.test.mjs": [`import { spawn } from "node:child_process";`, IMPORTS, PROCESS_DOOR],
  // The node:fs writers past the first list (review of PR #18): each creates or changes a file.
  "write-stream.mjs": [`import { WriteStream } from "node:fs";`, IMPORTS, WRITE_DOOR],
  "file-write-stream.mjs": [`import { FileWriteStream } from "fs";`, IMPORTS, WRITE_DOOR],
  "utf8-stream.mjs": [`import { Utf8Stream } from "node:fs";`, IMPORTS, WRITE_DOOR],
  "mkdtemp-disposable.mjs": [`import { mkdtempDisposableSync } from "fs";`, IMPORTS, WRITE_DOOR],
  "lchmod.mjs": [`import { lchmod } from "node:fs";`, IMPORTS, WRITE_DOOR],
  "lchmod-sync.mjs": [`import { lchmodSync } from "fs";`, IMPORTS, WRITE_DOOR],
  // Modules outside node:fs that write files. No door needs them, so every file bans them.
  "sqlite.mjs": [`import { DatabaseSync } from "node:sqlite";`, IMPORTS, OTHER_WRITER],
  "v8.mjs": [`import { writeHeapSnapshot } from "v8";`, IMPORTS, OTHER_WRITER],
  "trace-events.mjs": [`import { createTracing } from "node:trace_events";`, IMPORTS, OTHER_WRITER],
  "wasi.mjs": [`import { WASI } from "wasi";`, IMPORTS, OTHER_WRITER],
  "write-report.mjs": [`process.report.writeReport("r.json");`, PROPERTIES, "writeReport"],
  "heap-snapshot.mjs": [`globalThis.v8.writeHeapSnapshot("h");`, PROPERTIES, "writeHeapSnapshot"],
  "buildtime-proc.test.mjs": [`import { DatabaseSync } from "node:sqlite";`, IMPORTS, OTHER_WRITER],
};

/** The rules of the bans; the planted files break others (unused imports) that do not count. */
const BANS = new Set([
  IMPORTS,
  PROPERTIES,
  "eslint(no-eval)",
  "eslint(no-new-func)",
  "eslint(no-warning-comments)",
]);

/** Reads of node:fs, which every script may import. */
const READS = {
  "reads.mjs": `import { existsSync, readFileSync, readdirSync, statSync } from "node:fs";`,
};

/** Each door file's own door, which it may use. */
const DOORS = {
  "guard.mjs": `import { mkdirSync, renameSync, writeFileSync } from "node:fs";`,
  "buildtime-proc.mjs": `import { spawn } from "node:child_process";`,
  "buildtime-leader.mjs": `import { spawn } from "child_process";`,
  "buildtime-proc.test.mjs": `import { spawn } from "node:child_process";\nimport { rmSync } from "node:fs";`,
  "buildtime-lib.test.mjs": `import { mkdtempSync, writeFileSync } from "node:fs";`,
  "guard.test.mjs": `import { mkdirSync, rmSync, writeFileSync } from "node:fs";`,
  "oxlint-bans.test.mjs": `import { mkdtempSync, writeFileSync } from "node:fs";`,
};

/**
 * Ways around the bans that oxlint 1.87.0 cannot see, pinned so that a release that catches one
 * fails here and the PR's Not verified list shrinks. A dynamic import of node:fs: oxlint skips
 * every path with `importNames` for a dynamic import (no_restricted_imports.rs,
 * `report_string_literal_source`), and node:fs cannot be banned whole, as its reads stay allowed.
 * A disable comment that names no rule: it turns off every rule below it, no-warning-comments
 * included, and oxlint has no setting that ignores such comments.
 */
const UNCAUGHT = {
  "fs-dynamic.mjs": `const { writeFileSync } = await import("node:fs");`,
  "disable-all.mjs": `/* ${"oxlint"}-disable */\nimport { writeFileSync } from "node:fs";`,
};

const dirs = [];

/**
 * Lints `files` (a name under scripts/ to its text) in a scratch tree under the OS temp dir that
 * holds a copy of the app's .oxlintrc.json. Resolves with the diagnostics of the bans, by file.
 */
async function lint(files) {
  const dir = mkdtempSync(join(tmpdir(), "oxlint-bans-"));
  dirs.push(dir);
  writeFileSync(join(dir, ".oxlintrc.json"), readFileSync(join(APP_DIR, ".oxlintrc.json")));
  mkdirSync(join(dir, "scripts"));
  for (const [name, text] of Object.entries(files)) {
    writeFileSync(join(dir, "scripts", name), `${text}\nexport {};\n`);
  }
  const oxlint = resolveBin("oxlint", [APP_DIR]);
  const result = await runTool([oxlint, "--format", "json", "scripts"], {
    cwd: dir,
    timeoutMs: OXLINT_TIMEOUT_MS,
  });
  expect(result.timedOut, result.output).toBe(false);
  const byFile = {};
  const { diagnostics } = JSON.parse(result.output);
  // A file oxlint cannot parse gets a diagnostic with no rule, and none of the bans' rules.
  expect(diagnostics.filter((diagnostic) => diagnostic.code === undefined)).toEqual([]);
  for (const diagnostic of diagnostics) {
    if (BANS.has(diagnostic.code)) {
      const name = diagnostic.filename.replaceAll("\\", "/").replace(/^scripts\//, "");
      (byFile[name] ??= []).push(diagnostic);
    }
  }
  return byFile;
}

afterAll(() => {
  for (const dir of dirs.splice(0)) {
    rmSync(dir, { recursive: true, force: true });
  }
});

describe("the oxlint bans under scripts/", () => {
  let findings;

  beforeAll(async () => {
    const banned = Object.entries(BANNED).map(([name, [text]]) => [name, text]);
    findings = await lint({ ...Object.fromEntries(banned), ...READS, ...UNCAUGHT });
  }, OXLINT_TIMEOUT_MS);

  it.each(Object.entries(BANNED))("report %s", (name, [, code, door]) => {
    const found = (findings[name] ?? []).filter((diagnostic) => diagnostic.code === code);
    expect(found, JSON.stringify(findings[name] ?? [])).toHaveLength(1);
    expect(`${found[0].message} ${found[0].help ?? ""}`).toContain(door);
  });

  it("pass reads of node:fs", () => {
    expect(findings["reads.mjs"]).toBeUndefined();
  });

  it.each(Object.keys(UNCAUGHT))("still miss %s, which oxlint 1.87.0 cannot see", (name) => {
    expect(findings[name]).toBeUndefined();
  });
});

describe("the door files", () => {
  it(
    "may each use their own door",
    async () => {
      expect(await lint(DOORS)).toEqual({});
    },
    OXLINT_TIMEOUT_MS,
  );
});
