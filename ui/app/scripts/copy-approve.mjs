// Records Rohan's answer to the page of the English not yet approved (copy-running.mjs --english): every entry in
// ui/app/copy-approvals/ whose "status" is not "approved" (provisional, or a change asked for) becomes approved.
//
//   pnpm -C ui/app copy:approve --all
//   pnpm -C ui/app copy:approve --all --except UC-12,UD2-03             (those two stay as they are)
//   pnpm -C ui/app copy:approve --all --except UC-12,UD2-03 --change    (those two become "status": "change")
//
// Each entry it changes gets "at": today and "by": "Rohan (review page, <today>)"; only "status", "at" and "by" change.
// The Hindi ("hi", "hi_status", "hi_at", "hi_by") and the "note" are never touched. A file is written back with its
// keys in their order and the two-space layout it has, so the diff is those lines alone; a file that isn't in that
// layout is refused before anything is written. An --except id that isn't waiting for approval is refused too, so a
// typo can't pass. It prints a summary and commits nothing.
//
// --approvals <dir> points it at another folder (a scratch copy); --date YYYY-MM-DD sets "today".
import { readdirSync, readFileSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

/** The two-space layout every approvals file has: what JSON.stringify gives, and a newline. */
export const layout = (json) => `${JSON.stringify(json, null, 2)}\n`;

const DATE = /^\d{4}-\d{2}-\d{2}$/;

/** Today in this machine's time zone, as YYYY-MM-DD. */
export const today = (now = new Date()) =>
  `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, "0")}-${String(now.getDate()).padStart(2, "0")}`;

/**
 * Applies the answer to the files' texts ({ "U1.json": "<text>" }). Returns each changed file's new text and a
 * summary: { texts, approved: [[file, id]…], changed: [[file, id]…], kept: [[file, id]…] }. Throws, changing
 * nothing, on a file not in the layout or an --except id that isn't waiting for approval.
 */
export function approve(texts, { except = [], change = false, date }) {
  if (!DATE.test(date ?? "")) throw new Error(`the date must be YYYY-MM-DD, not ${date}`);
  const by = `Rohan (review page, ${date})`;
  const files = {};
  for (const [file, text] of Object.entries(texts)) {
    const json = JSON.parse(text);
    if (layout(json) !== text) {
      throw new Error(
        `${file} isn't in the two-space layout; format it first (oxfmt), so the diff stays small`,
      );
    }
    files[file] = json;
  }
  const waiting = new Map();
  for (const [file, json] of Object.entries(files))
    for (const [id, e] of Object.entries(json)) if (e.status !== "approved") waiting.set(id, file);
  const unknown = except.filter((id) => !waiting.has(id));
  if (unknown.length > 0) {
    throw new Error(`--except names ids that aren't waiting for approval: ${unknown.join(", ")}`);
  }
  const skip = new Set(except);
  const summary = { texts: {}, approved: [], changed: [], kept: [] };
  for (const [file, json] of Object.entries(files)) {
    let touched = false;
    for (const [id, e] of Object.entries(json)) {
      if (e.status === "approved") continue;
      if (skip.has(id) && !change) {
        summary.kept.push([file, id]);
        continue;
      }
      // assigning existing keys keeps their place; a missing "by" (an old change entry) goes last
      e.status = skip.has(id) ? "change" : "approved";
      e.at = date;
      e.by = by;
      (skip.has(id) ? summary.changed : summary.approved).push([file, id]);
      touched = true;
    }
    if (touched) summary.texts[file] = layout(json);
  }
  return summary;
}

/** The summary printed after a run. */
export function summaryText({ texts, approved, changed, kept }, dir) {
  const byFile = (list) => {
    const counts = new Map();
    for (const [file] of list) counts.set(file, (counts.get(file) ?? 0) + 1);
    return [...counts].map(([file, n]) => `${file} ${n}`).join(", ");
  };
  const lines = [
    `copy:approve: ${approved.length} approved, ${changed.length} set to "change", ${kept.length} left as they were, in ${Object.keys(texts).length} files under ${dir}.`,
  ];
  if (approved.length > 0) lines.push(`  approved: ${byFile(approved)}`);
  if (changed.length > 0) lines.push(`  change: ${changed.map(([, id]) => id).join(", ")}`);
  if (kept.length > 0) lines.push(`  left as they were: ${kept.map(([, id]) => id).join(", ")}`);
  lines.push("Nothing is committed; review the diff, then run copy:check.");
  return `${lines.join("\n")}\n`;
}

/** The command line's options: { all, except, change, dir, date }. Throws on anything it doesn't know. */
export function parseArgs(argv, defaultDir) {
  const o = { all: false, except: [], change: false, dir: defaultDir, date: today() };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    const value = () => {
      const v = argv[++i];
      if (v === undefined || v.startsWith("--")) throw new Error(`${a} needs a value`);
      return v;
    };
    if (a === "--all") o.all = true;
    else if (a === "--change") o.change = true;
    else if (a === "--except")
      o.except.push(
        ...value()
          .split(",")
          .map((s) => s.trim())
          .filter(Boolean),
      );
    else if (a === "--approvals") o.dir = value();
    else if (a === "--date") o.date = value();
    else throw new Error(`unknown option ${a}`);
  }
  if (!o.all)
    throw new Error("say --all: every English entry not yet approved, less any --except ids");
  if (o.change && o.except.length === 0) throw new Error("--change goes with --except <id,…>");
  return o;
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const app = fileURLToPath(new URL("..", import.meta.url));
  const o = parseArgs(process.argv.slice(2), join(app, "copy-approvals"));
  const names = readdirSync(o.dir)
    .filter((f) => f.endsWith(".json"))
    .sort();
  const texts = Object.fromEntries(names.map((f) => [f, readFileSync(join(o.dir, f), "utf8")]));
  const result = approve(texts, o);
  for (const [file, text] of Object.entries(result.texts)) writeFileSync(join(o.dir, file), text);
  process.stdout.write(summaryText(result, o.dir));
}
