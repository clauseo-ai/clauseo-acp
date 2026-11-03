// A tool call in the lawyer's words: what it touched, named as the lawyer knows it (the file's name, its folder kept for
// the hover), and what is being done now, for the working line and the live step group. The places come from the
// entry's `locations` (matter-relative, or outside the matter as display text) and its `raw_input`'s usual fields
// (file_path, path, query, pattern, url, description), and only then from the title, whose absolute paths are cut
// to their file names. Kept per entry object (an update makes a new one), so a flush reads them once. In the room's
// first chunk with the working line: small, with nothing of the settled group's words.
import type { Lang } from "@clauseo/components/lib/display.ts";
import { stepLive } from "@clauseo/components/lib/words-steps-live.ts";
import type { StreamView } from "../../store/view.ts";
import type { ToolCallEntry } from "../../transport/protocol.ts";
import { turnRunning } from "./live.ts";

/** What a step touched. `name`: the paper's file name, a site, or null; `where`: its folder or full place, for the
 * hover; `query`: the words searched for; `what`: the agent's own short words for a task (a command's description). */
export type StepPlace = {
  name: string | null;
  where: string | null;
  query: string | null;
  what: string | null;
  outside: boolean;
};

const PATH_KEYS = ["file_path", "path", "notebook_path", "filePath", "file", "abs_path"] as const;
const QUERY_KEYS = ["query", "pattern", "q", "search"] as const;
/** Past this, raw input isn't parsed for a name: a long write's whole text is no place to look. */
const RAW_MAX = 16_384;
const SHORT = 120;

const lastSlash = (p: string) => Math.max(p.lastIndexOf("/"), p.lastIndexOf("\\"));

/** A path's file name, and its folder ("" at the top). A trailing slash is dropped first. */
export function splitPath(path: string): { name: string; folder: string } {
  const p = path.replace(/[/\\]+$/, "");
  const at = lastSlash(p);
  return at < 0 ? { name: p, folder: "" } : { name: p.slice(at + 1), folder: p.slice(0, at) };
}

const short = (s: unknown): string | null => {
  if (typeof s !== "string") return null;
  const t = s.trim().replace(/\s+/g, " ");
  if (t === "") return null;
  return t.length > SHORT ? `${t.slice(0, SHORT - 1)}…` : t;
};

function rawOf(entry: ToolCallEntry): Record<string, unknown> | null {
  const text = entry.raw_input?.inline;
  if (text === undefined || text.length > RAW_MAX || !text.startsWith("{")) return null;
  try {
    const v: unknown = JSON.parse(text);
    return typeof v === "object" && v !== null && !Array.isArray(v)
      ? (v as Record<string, unknown>)
      : null;
  } catch {
    return null;
  }
}

const first = (raw: Record<string, unknown> | null, keys: readonly string[]): string | null => {
  if (raw === null) return null;
  for (const k of keys) {
    const v = short(raw[k]);
    if (v !== null) return v;
  }
  return null;
};

function siteOf(url: string): { name: string; where: string } | null {
  try {
    const u = new URL(url);
    return u.hostname === "" ? null : { name: u.hostname.replace(/^www\./, ""), where: url };
  } catch {
    return null;
  }
}

/** An absolute path, Unix or Windows, as a run of non-space characters. Never a slash inside a word or after a colon
 * or another slash, so a web address ("https://indiankanoon.org/doc/1/") is no path and stays whole. */
const ABSOLUTE = /(?<![\w:/\\.])(?:[A-Za-z]:\\|\/)[^\s"'`]*[/\\][^\s"'`]+/g;
/** A web address in a title. */
const WEB = /\bhttps?:\/\/[^\s"'`]+/;

/** The agent's title with each absolute path cut to its file name: the fallback when nothing else names the step. */
export const titleOf = (title: string): string =>
  title.replace(ABSOLUTE, (p) => splitPath(p).name || p).trim();

const places = new WeakMap<ToolCallEntry, StepPlace>();

/** What a step touched, as the lawyer reads it. */
export function placeOf(entry: ToolCallEntry): StepPlace {
  const known = places.get(entry);
  if (known !== undefined) return known;
  const raw = rawOf(entry);
  const place: StepPlace = {
    name: null,
    where: null,
    query: first(raw, QUERY_KEYS),
    what: first(raw, ["description"]),
    outside: false,
  };
  const at = entry.locations.find((l) => typeof l === "object");
  const located = typeof at === "object" ? (at.inside?.path ?? at.outside?.display ?? null) : null;
  const url = first(raw, ["url"]);
  const site = url === null ? null : siteOf(url);
  const path = located ?? first(raw, PATH_KEYS);
  if (path !== null) {
    const { name, folder } = splitPath(path);
    place.name = name === "" ? null : name;
    place.where = folder === "" ? null : folder;
    place.outside = typeof at === "object" && at.outside !== undefined;
  } else if (site !== null) {
    place.name = site.name;
    place.where = site.where;
  } else {
    // nothing names it but the agent's title, whose paths the row cuts to their file names: the first one's folder is
    // the hover, so where the paper is stays a pointer away; with no path, a web address in it, whole
    const named = entry.title.match(ABSOLUTE)?.[0];
    const folder = named === undefined ? "" : splitPath(named).folder;
    place.where = folder === "" ? (entry.title.match(WEB)?.[0] ?? null) : folder;
  }
  places.set(entry, place);
  return place;
}

/** The agent's title as a sentence, its paths cut, where nothing else names the step; `plain` with no title. */
export function titled(entry: ToolCallEntry, plain: string): string {
  const title = titleOf(entry.title);
  return title === "" ? plain : sentenceOf(title);
}

/** What a step is doing now, in a sentence: "Reading Lease.pdf.", "Searching for 'notice period'.", or the agent's
 * title with its paths cut, for a kind with no words of its own. */
export function doingOf(lang: Lang, entry: ToolCallEntry): string {
  const p = placeOf(entry);
  const name = p.name;
  switch (entry.kind) {
    case "read":
      return name === null
        ? titled(entry, stepLive(lang, "Reading the papers."))
        : stepLive(lang, "Reading {name}.", { name });
    case "edit":
      return name === null
        ? titled(entry, stepLive(lang, "Changing a paper."))
        : stepLive(lang, "Changing {name}.", { name });
    case "delete":
      return name === null
        ? titled(entry, stepLive(lang, "Deleting a paper."))
        : stepLive(lang, "Deleting {name}.", { name });
    case "move":
      return name === null
        ? titled(entry, stepLive(lang, "Moving a paper."))
        : stepLive(lang, "Moving {name}.", { name });
    case "search":
      return p.query === null
        ? titled(entry, stepLive(lang, "Searching the papers."))
        : stepLive(lang, "Searching for '{query}'.", { query: p.query });
    case "fetch":
      return p.query !== null
        ? stepLive(lang, "Searching online for '{query}'.", { query: p.query })
        : name !== null
          ? stepLive(lang, "Reading {name} online.", { name })
          : titled(entry, stepLive(lang, "Looking online."));
    case "execute":
      return p.what === null ? stepLive(lang, "Running a task.") : sentenceOf(p.what);
    case "think":
      return stepLive(lang, "Thinking it through.");
    default:
      return titled(entry, stepLive(lang, "Working on it."));
  }
}

/** The agent's own words as a sentence: a full stop where it has no end of its own. */
export const sentenceOf = (s: string): string => (/[.!?।…]$/.test(s) ? s : `${s}.`);

/** Whether a step is still to finish. */
export const unsettled = (entry: ToolCallEntry): boolean =>
  entry.status === "pending" || entry.status === "in_progress";

const TOOL = "tool_call:";
const TEXT = "agent_text:";

/** The working line's words while a turn runs: what its newest step is doing ("Reading Lease.pdf."), "Thinking it
 * through." while its newest output is a thought, and null while it writes the answer, has finished its newest step,
 * or nothing runs. A string, so a flush that leaves it the same renders nothing. */
export function doingPick(lang: Lang): (v: StreamView | undefined) => string | null {
  return (v) => {
    if (v === undefined || !turnRunning(v.header?.header?.status)) return null;
    for (let i = v.ids.length - 1; i >= 0; i -= 1) {
      const id = v.ids[i]!;
      if (id.startsWith(TOOL)) {
        const call = v.entries.get(id)?.tool_call;
        return call !== undefined && unsettled(call) ? doingOf(lang, call) : null;
      }
      if (id.startsWith(TEXT))
        return v.entries.get(id)?.agent_text?.kind === "thought"
          ? stepLive(lang, "Thinking it through.")
          : null;
      if (id.startsWith("user_message:") || id.startsWith("step:")) return null;
    }
    return null;
  };
}

/** While the turn waits on the lawyer (needs_you): a pending permission prompt, or else a question; null otherwise. The
 * working line says so in place of what the step in hand is doing, and the pen stops. */
export function asksPick(v: StreamView | undefined): "permission" | "question" | null {
  if (v?.header?.header?.status !== "needs_you") return null;
  for (const entry of v.entries.values())
    if (entry.permission !== undefined && entry.permission.state === "pending") return "permission";
  return "question";
}
