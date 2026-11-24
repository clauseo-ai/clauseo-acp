// What a row and a detail say about an item, from its list_addons fields: its name and line (a
// built-in's are the app's own words), where it came from, where it is on, and what stands in its
// way.
import type { Lang } from "@clauseo/components/lib/display.ts";
import { listOf } from "@clauseo/components/lib/words-core.ts";
import { cr } from "@clauseo/components/lib/words-connectors-room.ts";
import { lb, type LibraryKey } from "@clauseo/components/lib/words-library.ts";
import { sk } from "@clauseo/components/lib/words-skills.ts";
import { CASE_LAW } from "../store/connectors.ts";
import type { MatterId } from "../transport/protocol.ts";
import type { AddonStatus, AddonView } from "./addons.ts";
import type { CaseLawFacts } from "./addons-source.ts";

export type Part = "skills" | "connectors";

/** Which items a part lists. agent-browser is listed by the host but named in the Web browser's
 * detail instead. */
export const inPart = (part: Part, a: AddonView): boolean =>
  a.id !== "agent-browser" && (part === "skills" ? a.kind === "skill" : a.kind === "tool");

/** What the lawyer has, for the rail's count: every skill listed, and the connectors that are
 * their own or added (Clauseo Case Law once added and on); not the built-in browser. */
export function haveCount(
  items: readonly AddonView[],
  caseLaw: CaseLawFacts,
): { skills: number; connectors: number } {
  const listed = items.filter((a) => a.id !== "agent-browser");
  const added = (a: AddonView) =>
    a.source === "added" ||
    (a.id === CASE_LAW &&
      caseLaw.off === null &&
      (a.status === "ready" || caseLaw.credits !== null));
  return {
    skills: listed.filter((a) => a.kind === "skill").length,
    connectors: listed.filter((a) => a.kind === "tool" && added(a)).length,
  };
}

/** The built-ins the app names in its own words: their name and their one line. */
const BUILT_IN: Record<string, { name: LibraryKey; line: LibraryKey }> = {
  "legal-drafting": {
    name: "Legal drafting",
    line: "Drafts and revises notices, replies, opinions and agreements from the matter's papers, and checks every citation before handing the draft over.",
  },
  "legal-research": {
    name: "Legal research",
    line: "Researches Indian law over several searches, reads the later judgments that cite a case, and ties every quote to a page it read.",
  },
  "matter-brief": {
    name: "Matter brief",
    line: "Keeps a short brief of the matter in its notes folder: parties, facts, forum, positions taken and open questions.",
  },
  stetdoc: {
    name: "Word documents",
    line: "Opens and edits Word files in the matter, with each change shown as a tracked change you can accept or reject.",
  },
  browser: {
    name: "Web browser",
    line: "Opens web pages that need a real browser, such as court websites with forms to fill.",
  },
};

/** The item's name: the app's words for a built-in it knows, else the host's. */
export function nameOf(lang: Lang, a: AddonView): string {
  if (a.id === CASE_LAW) return cr(lang, "Clauseo Case Law");
  const own = a.source === "built_in" ? BUILT_IN[a.id] : undefined;
  return own === undefined ? a.name : lb(lang, own.name);
}

/** Its description: the app's words for a built-in it knows, else the host's line. */
export function descriptionOf(lang: Lang, a: AddonView): string {
  if (a.id === CASE_LAW)
    return cr(
      lang,
      "Find and read Supreme Court, High Court and tribunal judgments and central Acts, and see who cites each judgment.",
    );
  const own = a.source === "built_in" ? BUILT_IN[a.id] : undefined;
  return own === undefined ? a.description : lb(lang, own.line);
}

const hostOf = (url: string): string => {
  try {
    return new URL(url).host;
  } catch {
    return url;
  }
};

/** Where it came from, in the lawyer's words; for an added tool, "MCP server" said. */
export function fromLine(lang: Lang, a: AddonView): string {
  if (a.source === "built_in") return lb(lang, "Comes with Clauseo");
  const f = a.from;
  if (f === undefined) return lb(lang, "Added by you");
  switch (f.kind) {
    case "skills_sh":
      return lb(lang, "From {source}", { source: `skills.sh, ${f.text}` });
    case "github":
      return lb(lang, "From {source}", {
        source: `GitHub, ${f.text.replace(/^github\.com\//, "")}`,
      });
    case "url":
      return lb(lang, "From {source}", { source: hostOf(f.text) });
    case "zip":
      return lb(lang, "From a zip file on this computer");
    case "folder":
      return lb(lang, "From a folder on this computer");
    case "program":
      return lb(lang, "MCP server, a program on this computer");
    case "address":
      return lb(lang, "MCP server at {host}", { host: hostOf(f.text) });
    case "import":
      return lb(lang, "MCP server, brought over from {app}", { app: f.text });
  }
}

/** The line under the name: the description, or, for an added item without one, where it came from.
 */
export function rowLine(lang: Lang, a: AddonView): string {
  const line = descriptionOf(lang, a);
  return line === "" ? fromLine(lang, a) : line;
}

/** Where it is on, as the row's short tag says it: "Every matter", "Only Sharma v. State of Delhi",
 * or turned off. Scope "matter" names its matters when the host lists them (stage D's `switched`),
 * and says nothing otherwise. */
export function scopeTag(lang: Lang, a: AddonView, names: ReadonlyMap<MatterId, string>): string {
  if (a.scope === "all") return a.on ? lb(lang, "Every matter") : lb(lang, "Turned off");
  const on = (a.switched ?? []).map((id) => names.get(id)).filter((n) => n !== undefined);
  return on.length === 0 ? "" : lb(lang, "Only {matters}", { matters: listOf(lang, on) });
}

/** The row's word in place of where it is on, when something is in the way; null when nothing is.
 * Case Law's statuses show on its own card. */
export function troubleTag(lang: Lang, status: AddonStatus): string | null {
  switch (status) {
    case "changed":
      return lb(lang, "Stopped");
    case "invalid":
      return lb(lang, "Can't be used");
    case "blocked":
      return lb(lang, "Not allowed");
    case "needs_secret":
      return lb(lang, "Key missing");
    default:
      return null;
  }
}

/** What stands in the item's way, said in full for its detail; null when nothing does. */
export function troubleLine(lang: Lang, status: AddonStatus, who: string): string | null {
  switch (status) {
    case "changed":
      return lb(lang, "Its files changed after you added it, so Clauseo stopped using it.");
    case "invalid":
      return sk(lang, "Clauseo can't use this skill as it is.");
    case "blocked":
      return lb(lang, "Your firm doesn't allow this one on this computer.");
    case "needs_secret":
      return lb(lang, "Its saved key is missing, so {who} can't reach it.", { who });
    case "not_offered":
      return lb(lang, "Not available yet.");
    case "unreachable":
      return lb(
        lang,
        "Clauseo can't reach Clauseo Case Law right now, so {who} can't search judgments.",
        {
          who,
        },
      );
    default:
      return null;
  }
}
