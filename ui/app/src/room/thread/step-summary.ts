// The folded line's summary of a settled step group: what ran, named, in one sentence ("Read 3 papers, searched the
// papers, used Indian Kanoon, used the legal drafting skill, ran 2 commands."), never a count of unnamed things
// (issue 13; until 24 Nov a sentence a kind, the three done most, "Did 3 other things. Ran 1 task."). Pure.
import type { Lang } from "@clauseo/components/lib/display.ts";
import { steps, type StepKey } from "@clauseo/components/lib/words-steps.ts";
import type { ToolCallEntry } from "../../transport/protocol.ts";
import { groupKindOf } from "./group-kind.ts";
import { helperOf } from "./helper-words.ts";
import { skillOf } from "./skill-words.ts";
import { placeOf, siteQuery } from "./step-names.ts";
import {
  mcpOf,
  readableServer,
  readableTool,
  toolLabelOf,
  toolNameOf,
  yieldOf,
} from "./tool-names.ts";

/** The tools that list a folder rather than read a paper (Claude's LS and Glob, Gemini's list_directory and the like),
 * by their name in lower case. */
const LISTERS: ReadonlySet<string> = new Set([
  "ls",
  "glob",
  "list",
  "list_dir",
  "listdir",
  "list_directory",
  "list_files",
  "read_folder",
  "readfolder",
]);

/** Whether a read step lists a folder: a lister's tool (LISTERS), a title that runs one (`ls papers`, `Glob **` or
 * Codex's "List papers", which names no file), or a path that ends in a folder's separator. */
function listingOf(call: ToolCallEntry): boolean {
  if (call.tool_name !== undefined && LISTERS.has(call.tool_name.toLowerCase())) return true;
  const title = call.title.trim().replace(/^`+/, "");
  if (/^(?:ls|glob|dir|tree)(?:\s|$)/i.test(title)) return true;
  if (/^list\b/i.test(title) && !/\.\w{1,5}`?$/.test(title)) return true;
  const raw = rawPathOf(call);
  return raw !== null && /[/\\]$/.test(raw);
}

/** A step's path as its locations or its input give it, as it came (a folder's trailing separator kept), or null. */
function rawPathOf(call: ToolCallEntry): string | null {
  const at = call.locations.find((l) => typeof l === "object");
  if (typeof at === "object") return at.inside?.path ?? at.outside?.display ?? null;
  const text = call.raw_input?.inline;
  if (text === undefined || text.length > 16_384 || !text.startsWith("{")) return null;
  try {
    const v = JSON.parse(text) as Record<string, unknown>;
    const p = v["file_path"] ?? v["path"] ?? v["filePath"] ?? v["dir_path"];
    return typeof p === "string" ? p : null;
  } catch {
    return null;
  }
}

/** How many papers a group's read steps read: each paper once, however often it was read; a read of a guide
 * (skill-words.ts) or a folder's listing (listingOf) not at all; a read that names no paper once, as each did before. */
export function papersRead(calls: readonly ToolCallEntry[]): number {
  const papers = new Set<string>();
  let unnamed = 0;
  for (const c of calls) {
    if (groupKindOf(c) !== "read" || skillOf(c) !== null || listingOf(c)) continue;
    const p = placeOf(c);
    if (p.name === null) unnamed += 1;
    else papers.add(`${p.where ?? ""}/${p.name}`);
  }
  return papers.size + unnamed;
}

/** What a summary says of a group's steps of one sort, from what it gathered: how many, and the names. */
type Gathered = { n: number; names: string[]; unnamed: number };

/** The sorts a summary tells apart: each paper kind, the searches, looking online, each case-law family, commands,
 * skills, helpers, the tools named by their own name ("used"), and planning. */
type Sort =
  | "read"
  | "edit"
  | "delete"
  | "move"
  | "search"
  | "fetch"
  | "case_law_search"
  | "case_law_credits"
  | "case_law_payment"
  | "execute"
  | "skill"
  | "helper"
  | "used"
  | "wait"
  | "think";

/** A step's sort, and the name it brings, if any: a skill's, a tool's, a site's. */
function sortOf(c: ToolCallEntry): { sort: Sort; name: string | null } {
  if (helperOf(c)) return { sort: "helper", name: null };
  if (yieldOf(c)) return { sort: "wait", name: null };
  const skill = skillOf(c);
  if (skill !== null)
    return { sort: "skill", name: skill.name === "" ? null : readableTool(skill.name) };
  const mcp = mcpOf(c.title, c.raw_input?.inline);
  if (mcp !== null) return { sort: "used", name: readableServer(mcp.server) };
  const k = groupKindOf(c);
  if (k === "fetch") {
    const p = placeOf(c);
    const site = p.query === null ? null : siteQuery(p.query).site;
    return { sort: "fetch", name: site ?? (p.query === null ? p.name : null) };
  }
  // a tool's name the room doesn't know, under any kind but a paper's, is told by that name
  const unknown = toolNameOf(c.title)?.as === null;
  if (k === "other" || (k === "execute" && unknown)) return { sort: "used", name: toolLabelOf(c) };
  return { sort: k, name: null };
}

/** The words of a sort counted, one and many. */
const COUNTED: Partial<Record<Sort, [one: StepKey, many: StepKey]>> = {
  read: ["read 1 paper", "read {n} papers"],
  edit: ["changed 1 paper", "changed {n} papers"],
  delete: ["deleted 1 paper", "deleted {n} papers"],
  move: ["moved 1 paper", "moved {n} papers"],
  execute: ["ran 1 command", "ran {n} commands"],
  helper: ["asked a helper", "asked {n} helpers"],
  case_law_search: ["searched case law", "searched case law {n} times"],
  case_law_credits: ["checked your case-law credits", "checked your case-law credits {n} times"],
  case_law_payment: [
    "got a payment link for case-law credits",
    "got {n} payment links for case-law credits",
  ],
};

/** Names joined as the language lists them ("Indian Kanoon and SCC Online"). */
const listed = (lang: Lang, names: readonly string[]): string =>
  new Intl.ListFormat(lang === "hi" ? "hi" : "en", { type: "conjunction" }).format(names);

/** A sort's words in the summary, from what it gathered. */
function phraseOf(lang: Lang, sort: Sort, { n, names, unnamed }: Gathered): string {
  const counted = COUNTED[sort];
  if (counted !== undefined)
    return n === 1 ? steps(lang, counted[0]) : steps(lang, counted[1], { n });
  switch (sort) {
    case "search":
      return steps(lang, "searched the papers");
    case "fetch":
      return names.length === 0
        ? steps(lang, "looked online")
        : steps(lang, "looked at {sites} online", { sites: listed(lang, names) });
    case "skill":
      // one skill by its name; more, or one that names none, counted
      if (n === 1 && names.length === 1)
        return steps(lang, "used the {skill} skill", { skill: names[0]! });
      return n === 1 ? steps(lang, "used 1 skill") : steps(lang, "used {n} skills", { n });
    case "used": {
      const shown =
        names.length === 0 ? [] : [steps(lang, "used {names}", { names: listed(lang, names) })];
      // a step no name tells, said once beside the names
      if (unnamed > 0) shown.push(steps(lang, "used a tool"));
      return shown.join(", ");
    }
    case "wait":
      // however often Claude waited, said once
      return steps(lang, "waited for the background work");
    default:
      return steps(lang, "planned the work");
  }
}

/** The folded line's summary: each sort of what finished, the most first, ties in the order each came, in one
 * sentence. Reads count papers (papersRead), not steps; with no paper among them (only guides or listings) the read
 * words are left out, unless they are all the line has, when they count the steps as before. */
export function summaryOf(lang: Lang, calls: readonly ToolCallEntry[]): string {
  const by = new Map<Sort, Gathered>();
  for (const c of calls) {
    const { sort, name } = sortOf(c);
    const g = by.get(sort) ?? { n: 0, names: [], unnamed: 0 };
    g.n += 1;
    if (name === null) g.unnamed += 1;
    else if (!g.names.includes(name)) g.names.push(name);
    by.set(sort, g);
  }
  const read = by.get("read");
  if (read !== undefined) {
    const papers = papersRead(calls);
    if (papers > 0) read.n = papers;
    else if (by.size > 1) by.delete("read");
  }
  // Map keeps the order each sort came; sort is stable
  const parts = [...by].sort((a, b) => b[1].n - a[1].n).map(([sort, g]) => phraseOf(lang, sort, g));
  return partsLine(lang, parts);
}

/** The summary's parts as one sentence: joined with commas, the first letter a capital in English, and the stop. */
export function partsLine(lang: Lang, parts: readonly string[]): string {
  const out = parts.join(", ");
  if (out === "") return "";
  const first = lang === "en" ? out[0]!.toUpperCase() : out[0]!;
  return `${first}${out.slice(1)}${lang === "hi" ? "।" : "."}`;
}
