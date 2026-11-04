// Find's results, shaped for the rail (U16a, the approved page's findView and findContent): pure, no React. Above, while
// the host's index is still being built, RO-27's line. Then "Matching matters": the matters matched on the rail by
// name, client or case number (each with the line that says why), then the matters the host found by title that those
// don't already hold, then the papers found by file name (RO-25). Then "Conversations" (RO-21): those found by title,
// then those found by what was said in them (a prompt is the lawyer's, "You:"; an answer or a tool's title Clauseo's,
// "Clauseo:", RO-24), each with its matter and day. A snippet stays text: its found words are marked by cutting it into
// parts, never as markup. A title the host doesn't have yet reads as the room's own words for it. A hit of a kind this
// room doesn't know is left out.
import type { MatterRowProps } from "@clauseo/components/clauseo/rail.tsx";
import type { Lang } from "@clauseo/components/lib/display.ts";
import { fw } from "@clauseo/components/lib/words-find.ts";
import { mt } from "@clauseo/components/lib/words-matters.ts";
import { t } from "@clauseo/components/lib/words.ts";
import type { MatterId, SnippetPart, ThreadId } from "../transport/protocol.ts";
import type { SearchState } from "../store/search.ts";
import type { AllThreadsView } from "./all-threads.ts";
import type { ConversationsView } from "./conversations-view.ts";
import type { MatterInfo } from "./host-next.ts";
import { clockOf } from "./when.ts";

/** A paper found by its file name: its matter, its path in the matter's folder, and the name, cut at the found words. */
export type FoundPaper = {
  key: string;
  matter: MatterId;
  /** The matter's name, the row's first line. */
  name: string;
  path: string;
  /** The file's name alone, for the row's accessible name. */
  file: string;
  parts: readonly SnippetPart[];
};

/** A conversation found by its title or by something said in it. */
export type FoundConversation = {
  key: string;
  matter: MatterId;
  thread: ThreadId;
  /** The entry the words were found in; none for a title. */
  order?: number;
  title: string;
  /** "{matter}, {day}", or the day alone where the matter's name isn't known. */
  where: string;
  /** Whose words were found: "You:" or "Clauseo:"; none for a title. */
  who?: string;
  /** The words around what was found, cut at the found words; none for a title. */
  parts?: readonly SnippetPart[];
  archived: boolean;
};

export type FindView = {
  /** RO-27: the host's index is still being built. */
  indexing: boolean;
  matters: readonly MatterRowProps[];
  papers: readonly FoundPaper[];
  conversations: readonly FoundConversation[];
  /** RO-26: the answer is in, and nothing at all was found. */
  nothing: boolean;
};

export type FindInput = {
  lang: Lang;
  /** The words in the field. */
  query: string;
  /** The matters the rail matched by name, client or case number, their rows ready. */
  matching: readonly MatterRowProps[];
  search: SearchState;
  /** Every matter the room lists, for names the host leaves out. */
  matters: readonly MatterInfo[];
  /** Each matter's row, for a matter the host found by its title. */
  rows: ReadonlyMap<MatterId, MatterRowProps>;
  /** Whether the room knows the thread to be archived (SearchHit carries no archived flag). */
  archived: (matter: MatterId, thread: ThreadId) => boolean;
  now: number;
  zone: string;
};

const CONTENT = { prompt: "You:", answer: "Clauseo:", tool_title: "Clauseo:" } as const;

/** The last part of a path: the file's name. */
const fileOf = (path: string) =>
  path.slice(Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\")) + 1);

export function findView(input: FindInput): FindView {
  const { lang, search, now, zone } = input;
  const byId = new Map(input.matters.map((m) => [m.matter, m]));
  /** A matter's name: as the room lists it, else as the host found it, else the scratch matter's own words. */
  const nameOf = (matter: MatterId, title: string | undefined): string | undefined => {
    const held = byId.get(matter);
    if (held?.kind === "scratch") return t(lang, "Scratch matter");
    return held?.name ?? (title === undefined || title === "" ? undefined : title);
  };
  const dayOf = (at: number) => clockOf(at, now, zone, lang).day || fw(lang, "today");
  const whereOf = (matter: MatterId, title: string | undefined, at: number) => {
    const name = nameOf(matter, title);
    const day = dayOf(at);
    return name === undefined ? day : fw(lang, "{matter}, {day}", { matter: name, day });
  };
  const untitled = mt(lang, "New conversation");
  // the hits shown answer the words in the field, or, until those are answered, the words just before
  const hits = search.answered === "" ? [] : search.hits;
  const shown = new Set<MatterId>();
  const matters: MatterRowProps[] = [];
  for (const row of input.matching) {
    if (row.id !== undefined) shown.add(row.id as MatterId);
    matters.push(row);
  }
  const papers: FoundPaper[] = [];
  const titles: FoundConversation[] = [];
  const said: FoundConversation[] = [];
  const titled = new Set<ThreadId>();
  hits.forEach((hit, i) => {
    switch (hit.kind) {
      case "matter_title": {
        const row = input.rows.get(hit.matter);
        if (row === undefined || shown.has(hit.matter)) return;
        shown.add(hit.matter);
        matters.push({ ...row, current: false, conversations: undefined });
        return;
      }
      case "file_name": {
        if (hit.path === undefined) return;
        const file = fileOf(hit.path);
        papers.push({
          key: `paper:${hit.matter}:${hit.path}`,
          matter: hit.matter,
          name: nameOf(hit.matter, hit.matter_title) ?? file,
          path: hit.path,
          file,
          parts: hit.snippet.length > 0 ? hit.snippet : [{ text: file, hit: false }],
        });
        return;
      }
      case "thread_title": {
        if (hit.thread === undefined || titled.has(hit.thread)) return;
        titled.add(hit.thread);
        titles.push({
          key: `title:${hit.thread}`,
          matter: hit.matter,
          thread: hit.thread,
          title: hit.thread_title || untitled,
          where: whereOf(hit.matter, hit.matter_title, hit.at),
          archived: input.archived(hit.matter, hit.thread),
        });
        return;
      }
      case "prompt":
      case "answer":
      case "tool_title": {
        if (hit.thread === undefined || hit.order === undefined) return;
        said.push({
          key: `said:${hit.thread}:${hit.order}:${i}`,
          matter: hit.matter,
          thread: hit.thread,
          order: hit.order,
          title: hit.thread_title || untitled,
          where: whereOf(hit.matter, hit.matter_title, hit.at),
          who: fw(lang, CONTENT[hit.kind]),
          parts: hit.snippet,
          archived: input.archived(hit.matter, hit.thread),
        });
        return;
      }
      default:
        // "unknown": a kind newer than this room
        return;
    }
  });
  const conversations = [...titles, ...said];
  const answered =
    search.settled && search.answered === search.query && search.query === input.query;
  return {
    indexing: search.indexing !== null,
    matters,
    papers,
    conversations,
    nothing: answered && matters.length === 0 && papers.length === 0 && conversations.length === 0,
  };
}

/** Whether a hit's thread is archived, as the room knows it: a hit carries no archived flag, so every matter's list
 * (all-threads.ts, archived threads included with that status) says; where the app feed hasn't given a matter's list,
 * the open matter's own list, which leaves its archived threads out, stands in. */
export const archivedIn =
  (every: AllThreadsView, talks: ConversationsView) =>
  (matter: MatterId, thread: ThreadId): boolean => {
    const known = every.lists.get(matter)?.find((t) => t.thread === thread);
    if (known !== undefined) return known.status === "archived";
    return (
      talks.matter === matter &&
      talks.phase === "ready" &&
      !talks.rows.some((r) => r.thread === thread)
    );
  };
