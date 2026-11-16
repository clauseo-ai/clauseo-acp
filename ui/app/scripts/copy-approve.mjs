// Records Rohan's answer to the page of the English not yet approved (copy-running.mjs --english): every entry in
// ui/app/copy-approvals/ whose "status" is not "approved" (provisional, or a change asked for) becomes approved.
//
//   pnpm -C ui/app copy:approve --all
//   pnpm -C ui/app copy:approve --all --except UC-12,UD2-03             (those two stay as they are)
//   pnpm -C ui/app copy:approve --all --except UC-12,UD2-03 --change    (those two become "status": "change")
//   pnpm -C ui/app copy:approve --only ids.txt [--except … [--change]]   (exactly the ids the file lists)
//
// --only <file> approves the ids its file lists and nothing else, so lines added after the page was built stay as they
// are. One id per line, blank lines and "#" comments allowed; after the id, a tab and the English the page showed, as
// a JSON string (the page's ids.txt has it). An id no file has fails the run. An id already approved is reported as
// already approved when its words are the ones listed, and fails the run when they aren't; an id waiting for approval
// whose words have changed since the list was made fails it too, since Rohan saw other words. --except ids must then
// be in the list.
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
 * The ids an --only file lists, in order, each with the English the page showed or null: [{ id, en }…]. Throws, naming
 * the line, on a line it can't read.
 */
export function parseIds(text) {
  const ids = [];
  text.split("\n").forEach((raw, n) => {
    const line = raw.replace(/\r$/, "");
    if (line.trim() === "" || line.trim().startsWith("#")) return;
    const tab = line.indexOf("\t");
    const id = (tab === -1 ? line.replace(/\s+#.*$/, "") : line.slice(0, tab)).trim();
    let en = null;
    if (tab !== -1) {
      try {
        en = JSON.parse(line.slice(tab + 1));
      } catch {
        en = undefined;
      }
    }
    if (id === "" || /\s/.test(id) || en === undefined || (en !== null && typeof en !== "string")) {
      throw new Error(
        `line ${n + 1} of the --only file isn't an id, or an id, a tab and its English as a JSON string: ${line}`,
      );
    }
    ids.push({ id, en });
  });
  return ids;
}

/**
 * Applies the answer to the files' texts ({ "U1.json": "<text>" }). Returns each changed file's new text and a
 * summary: { texts, approved, changed, kept, already: [[file, id]…] }. With only ([{ id, en }…], from parseIds) it
 * touches those ids alone. Throws, changing nothing, on a file not in the layout, an --except id that isn't waiting
 * for approval (or isn't listed), or an --only id that no file has or whose words aren't the ones listed.
 */
export function approve(texts, { except = [], change = false, date, only = null }) {
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
  const entries = new Map();
  for (const [file, json] of Object.entries(files))
    for (const [id, e] of Object.entries(json)) entries.set(id, { file, e });
  const summary = { texts: {}, approved: [], changed: [], kept: [], already: [] };
  // the ids this run may touch: every one waiting, or the listed ones
  let targets = null;
  if (only !== null) {
    const problems = [];
    targets = new Set();
    for (const { id, en } of only) {
      const found = entries.get(id);
      if (found === undefined) {
        problems.push(`${id}: no copy-approvals file has it`);
      } else if (en !== null && en !== found.e.en) {
        problems.push(
          `${id}: the list has ${JSON.stringify(en)}, but ${found.file} has ${JSON.stringify(found.e.en)} (${found.e.status})`,
        );
      } else if (found.e.status === "approved") {
        if (!summary.already.some(([, a]) => a === id)) summary.already.push([found.file, id]);
      } else {
        targets.add(id);
      }
    }
    if (problems.length > 0) {
      throw new Error(
        `--only: ${problems.length} listed ids can't be approved:\n  ${problems.join("\n  ")}`,
      );
    }
  }
  const unknown = except.filter((id) => {
    const found = entries.get(id);
    return (
      found === undefined || found.e.status === "approved" || (targets !== null && !targets.has(id))
    );
  });
  if (unknown.length > 0) {
    throw new Error(
      `--except names ids that aren't waiting for approval${targets === null ? "" : " in the --only list"}: ${unknown.join(", ")}`,
    );
  }
  const skip = new Set(except);
  for (const [file, json] of Object.entries(files)) {
    let touched = false;
    for (const [id, e] of Object.entries(json)) {
      if (e.status === "approved" || (targets !== null && !targets.has(id))) continue;
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
export function summaryText({ texts, approved, changed, kept, already = [] }, dir) {
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
  if (already.length > 0)
    lines.push(`  already approved: ${already.map(([, id]) => id).join(", ")}`);
  lines.push("Nothing is committed; review the diff, then run copy:check.");
  return `${lines.join("\n")}\n`;
}

/** The command line's options: { all, only, except, change, dir, date }; only is the --only file's path or null.
 * Throws on anything it doesn't know. */
export function parseArgs(argv, defaultDir) {
  const o = { all: false, only: null, except: [], change: false, dir: defaultDir, date: today() };
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
    else if (a === "--only") o.only = value();
    else if (a === "--approvals") o.dir = value();
    else if (a === "--date") o.date = value();
    else throw new Error(`unknown option ${a}`);
  }
  if (o.all === (o.only !== null))
    throw new Error(
      "say --all (every English entry not yet approved) or --only <file> (the ids it lists), not both, less any --except ids",
    );
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
  const only = o.only === null ? null : parseIds(readFileSync(o.only, "utf8"));
  const result = approve(texts, { ...o, only });
  for (const [file, text] of Object.entries(result.texts)) writeFileSync(join(o.dir, file), text);
  process.stdout.write(summaryText(result, o.dir));
}
