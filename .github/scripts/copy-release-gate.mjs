// The release gate for the lawyer's words: a v* release ships only words Rohan approved.
//
//   node .github/scripts/copy-release-gate.mjs [<copy-approvals folder>]
//
// Reads every ui/app/copy-approvals/*.json (or the folder given) and fails, with one ::error::
// line and every offending id grouped by reason, if any entry's `status` is not "approved" or any
// entry's Hindi is `"hi_status": "provisional"`. `"hi_status": "later"` passes: the Hindi is
// tracked, not a draft. copy:check (ui/app/src/review/copy-gate.check.ts) checks the entries'
// shape and that they match the registered words; this checks only that none is a draft.
// release.yml runs it in release mode only; copy-release-gate.test.mjs tests it. Plain Node.

import { readFileSync, readdirSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const DEFAULT_DIR = join(
  dirname(fileURLToPath(import.meta.url)),
  "..",
  "..",
  "ui",
  "app",
  "copy-approvals",
);

/** Every offending id, by reason, in file order then id order: { "<reason>": ["<id> (<file>)", …] }. */
export function offenders(files) {
  const groups = {};
  const add = (reason, line) => (groups[reason] ??= []).push(line);
  for (const [file, entries] of Object.entries(files).sort(([a], [b]) => a.localeCompare(b))) {
    if (entries === null || typeof entries !== "object" || Array.isArray(entries)) {
      add("not an object of entries", file);
      continue;
    }
    for (const [id, e] of Object.entries(entries)) {
      const status = e?.status;
      if (status !== "approved")
        add(`status ${JSON.stringify(status ?? null)}, not "approved"`, `${id} (${file})`);
      if (e?.hi_status === "provisional")
        add(`Hindi "hi_status": "provisional"`, `${id} (${file})`);
    }
  }
  return groups;
}

/** Each *.json in dir, parsed, by file name. */
export function readApprovals(dir) {
  const files = {};
  for (const name of readdirSync(dir)
    .filter((n) => n.endsWith(".json"))
    .sort()) {
    files[name] = JSON.parse(readFileSync(join(dir, name), "utf8"));
  }
  return files;
}

/** The gate's output and exit code for the files. */
export function report(files) {
  const groups = offenders(files);
  // The English's status first, then the Hindi's.
  const reasons = Object.keys(groups).sort(
    (a, b) => b.startsWith("status") - a.startsWith("status"),
  );
  if (reasons.length === 0) {
    const n = Object.values(files).reduce((sum, e) => sum + Object.keys(e).length, 0);
    return {
      code: 0,
      text: `copy release gate: all ${n} entries in ${Object.keys(files).length} files are approved\n`,
    };
  }
  const ids = new Set(Object.values(groups).flat());
  const lines = [
    `::error::copy release gate: ${ids.size} ${ids.size === 1 ? "entry is" : "entries are"} not approved by Rohan; ` +
      "a release ships only approved words (ui/app/copy-approvals)",
  ];
  for (const reason of reasons) {
    lines.push(`${reason}: ${groups[reason].length}`);
    for (const line of groups[reason]) lines.push(`  ${line}`);
  }
  return { code: 1, text: `${lines.join("\n")}\n` };
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const { code, text } = report(readApprovals(process.argv[2] ?? DEFAULT_DIR));
  process.stdout.write(text);
  process.exitCode = code;
}
