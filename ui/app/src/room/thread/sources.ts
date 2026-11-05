// The web sources of each answer: the searches the agent ran and the pages it opened, read off each tool call entry's
// `web` (ToolCallEntry, the reducer's view: the host records them, 0041). webPick gives an exchange's tool calls that
// carry one, in order, and the list (sources.tsx) draws them under the answer; `pagesShown` and the link helpers are
// the list's own. Until the host puts records on the log (S26 part 3), a real thread's tool calls carry no `web`, so
// the list draws nothing for them.
import { createContext } from "react";
import type { StreamView } from "../../store/view.ts";
import type { EntryId, ToolCallWeb, ViewEntry } from "../../transport/protocol.ts";
import { LINK_PREFIXES } from "@clauseo/components/markdown/schema.ts";
import { siteHost } from "@clauseo/components/clauseo/site-host.ts";
import type { HostTransport } from "../../transport/host-transport.ts";

/** A tool call's pages as the list shows them: each address once, where it first came, with the first title, or a
 * later one when the first had none. */
export function pagesShown(pages: ToolCallWeb["pages"]): ToolCallWeb["pages"] {
  const byUrl = new Map<string, ToolCallWeb["pages"][number]>();
  for (const page of pages) {
    const first = byUrl.get(page.url);
    if (first === undefined) byUrl.set(page.url, page);
    else if (first.title === undefined && page.title !== undefined)
      byUrl.set(page.url, { ...first, title: page.title });
  }
  return [...byUrl.values()];
}

/** The web of an exchange's tool calls, in order: each tool call entry in `tools` (its entry ids, from the window or an
 * older page) that carries `web`, with its id. A tool call with none is left out. The array is the same while its
 * members are: each is an entry id and the web object the view holds for it, which changes only when the entry does. */
export type ToolWeb = readonly [EntryId, ToolCallWeb];

const NO_WEB: readonly ToolWeb[] = [];

const sameWebs = (a: readonly ToolWeb[], b: readonly ToolWeb[]): boolean =>
  a.length === b.length && a.every((w, i) => w[0] === b[i]![0] && w[1] === b[i]![1]);

/** The picker of an exchange's tool calls' web (list.tsx): one per call here that carries web, given the same array
 * while none of them changed, so a flush that leaves them as they were renders nothing. */
export function webPick(
  tools: readonly EntryId[],
  paged: ReadonlyMap<EntryId, ViewEntry>,
): (v: StreamView | undefined) => readonly ToolWeb[] {
  let last = NO_WEB;
  return (v) => {
    const next: ToolWeb[] = [];
    for (const id of tools) {
      const web = (paged.get(id) ?? v?.entries.get(id))?.tool_call?.web;
      if (web !== undefined) next.push([id, web]);
    }
    if (!sameWebs(last, next)) last = next;
    return last;
  };
}

/** The transport the sources come from and their links open through: ThreadList's. */
export const SourcesTransport = createContext<HostTransport | null>(null);

/** Whether an answer's list starts open: false in the room, which shows it shut until pressed; the copy review sets
 * it so a reviewer reads the rows without a click (a press in its StrictMode render ran twice and shut it again). */
export const SourcesOpenAtFirst = createContext(false);

/** The schemes a source may link to: the web only, as an answer's links allow it (markdown/link.tsx) but without
 * mailto. The host logs a page's address as the agent gave it, so a data:, file: or javascript: address can come. */
const WEB_PREFIXES = LINK_PREFIXES.filter((prefix) => prefix.startsWith("http"));

/** Whether an address may sit under a link: http or https only. */
export const linkable = (url: string): boolean => {
  const lower = url.trimStart().toLowerCase();
  return WEB_PREFIXES.some((prefix) => lower.startsWith(prefix));
};

/** The words of a page's link: its title, or for an untitled page the address's file name (its last path segment,
 * percent-decoded), or else its site, so a link never reads as a whole address. An address with neither is itself. */
export function linkText(url: string, title: string | null): string {
  if (title !== null) return title;
  const site = siteHost(url);
  const path = /^[a-z][a-z\d+.-]*:\/\/[^/?#\\]*([^?#]*)/i.exec(url.trim())?.[1] ?? "";
  const last = path
    .split(/[/\\]/)
    .filter((part) => part !== "")
    .pop();
  if (last !== undefined) {
    try {
      return decodeURIComponent(last);
    } catch {
      return last;
    }
  }
  return site ?? url;
}
