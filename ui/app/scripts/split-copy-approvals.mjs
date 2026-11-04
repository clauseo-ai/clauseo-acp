// One-shot: splits the old single ui/app/copy-approvals.json into ui/app/copy-approvals/, one file per brief, named by
// the id's prefix before its first "-" (U1-01 in U1.json, UTC-12 in UTC.json; an id with no "-" in other.json). Each
// entry keeps its shape and its order within its file. An entry recorded as approved with a `by` naming "provisional"
// ("app track (provisional; Rohan batch review)") becomes "status": "provisional", its `by` kept as written, so it
// shows on the running page (copy-running.mjs) until Rohan looks.
//
// Rerun after a rebase, on the new main's file, from the repo root:
//   node ui/app/scripts/split-copy-approvals.mjs
// It reads ui/app/copy-approvals.json, writes the folder and deletes the old file. It stops if the folder already
// holds JSON files, unless --force, which removes them first. --from <file> and --to <dir> point it elsewhere;
// --keep leaves the old file in place.
import { existsSync, mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

/** The file an id's entry goes in: its prefix before the first "-", or other. */
export function fileOf(id) {
  const dash = id.indexOf("-");
  return `${dash <= 0 ? "other" : id.slice(0, dash)}.json`;
}

/** An entry as the folder keeps it: provisional if its `by` says so, otherwise as it was. */
export function entryOf(entry) {
  if (typeof entry?.by !== "string" || !/provisional/i.test(entry.by)) return entry;
  return { ...entry, status: "provisional" };
}

/** The old file's text split into the folder's files: file name to text, written as the old file was (two-space
 * JSON and a closing newline). */
export function splitApprovals(text) {
  const json = JSON.parse(text);
  if (typeof json !== "object" || json === null || Array.isArray(json))
    throw new Error("copy-approvals.json must be an object of id to approval");
  const files = new Map();
  for (const [id, entry] of Object.entries(json)) {
    const name = fileOf(id);
    if (!files.has(name)) files.set(name, {});
    files.get(name)[id] = entryOf(entry);
  }
  return new Map(
    [...files].map(([name, entries]) => [name, `${JSON.stringify(entries, null, 2)}\n`]),
  );
}

/** Splits `from` into `to`; returns the file names written, or null when there is no old file. */
export function splitFile({ from, to, force = false, keep = false }) {
  if (!existsSync(from)) return null;
  const files = splitApprovals(readFileSync(from, "utf8"));
  mkdirSync(to, { recursive: true });
  const there = readdirSync(to).filter((f) => f.endsWith(".json"));
  if (there.length > 0) {
    if (!force)
      throw new Error(
        `${to} already holds ${there.join(", ")}; rerun with --force to replace them`,
      );
    for (const f of there) rmSync(join(to, f));
  }
  for (const [name, text] of files) writeFileSync(join(to, name), text);
  if (!keep) rmSync(from);
  return [...files.keys()];
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const args = process.argv.slice(2);
  const opt = (name, fallback) => {
    const i = args.indexOf(name);
    return i === -1 ? fallback : args[i + 1];
  };
  const app = fileURLToPath(new URL("..", import.meta.url));
  const from = opt("--from", join(app, "copy-approvals.json"));
  const to = opt("--to", join(app, "copy-approvals"));
  const written = splitFile({
    from,
    to,
    force: args.includes("--force"),
    keep: args.includes("--keep"),
  });
  if (written === null) console.log(`${from} is not there: nothing to split`);
  else console.log(`${written.length} files in ${to}: ${written.join(", ")}`);
}
