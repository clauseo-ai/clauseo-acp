// The web sources of each answer: the searches the agent ran and the pages it opened, from the domain events
// `web_search_recorded` and `page_visited` (real binding types), folded per thread and tool call (foldWebSources) and kept
// per transport (sourcesOf) for sources.tsx's list under the answer. The fold gives each tool call the `web` the host is
// planned to put on its entry (ToolCallWeb), so the list reads the turn's tool calls either way.
//
// What makes it real: the desktop host sends a window no domain events. Today only FakeTransport (an EnvelopeSource)
// delivers them, so the real app has none and draws nothing. The host work that fills it is stage E's `web` on each
// ToolCallEntry, folded from these events on the view stream (then the list reads `entry.web` and the fold and the
// store go), or else an event subscription for windows that sends a thread's envelopes on the channel's `events` arm
// (then TauriTransport becomes an EnvelopeSource). Which events
// exist at all is the host's: S26 records WebFetch, Codex's open page and the searches; #275 records a page opened in
// the browser (`via: browser`).
import { createContext, useSyncExternalStore } from "react";
import type { EnvelopeSource, HostTransport } from "../../transport/host-transport.ts";
import type { StreamView } from "../../store/view.ts";
import type {
  EntryId,
  Envelope,
  PageVia,
  PageVisited,
  Seq,
  StreamId,
  ToolCallEntry,
  ToolCallId,
  ViewEntry,
  WebSearchRecorded,
} from "../../transport/protocol.ts";
import { LINK_PREFIXES } from "@clauseo/components/markdown/schema.ts";

/**
 * What a tool call found on the web, the shape the host is planned to give `ToolCallEntry` as `web` (stage E folds
 * `web_search_recorded` and `page_visited` into the tool call they name, on the view stream). Until it does, the
 * fold below builds the same object from the envelopes; the switch is to delete the fold and read `entry.web`. It
 * names no agent: the thread's agent ran it.
 */
export type ToolCallWeb = {
  /** The tool call's search, when it searched: a later search on the same tool call replaces it. Its results come in
   * the order the agent got them; none for none (and always none for Codex). */
  search?: { query: string; results: { title: string; url: string }[] };
  /** The pages it opened, appended in order, a page opened twice kept twice (pagesShown says each once); at most
   * MAX_PAGES. */
  pages: { url: string; title?: string; via: PageVia }[];
};

/** The most pages a tool call keeps, as the host's fold does: the first ones. */
export const MAX_PAGES = 50;

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

/** A tool call entry as it will read once the host fills `web`. */
export type ToolCallView = ToolCallEntry & { web?: ToolCallWeb };

/** Each thread's tool calls' web, by stream, then by tool call id. */
export type WebSources = ReadonlyMap<StreamId, ReadonlyMap<ToolCallId, ToolCallWeb>>;

const VIAS: readonly PageVia[] = ["web_fetch", "codex_open_page", "browser"];

/** The envelopes' web, per thread and tool call, each tool call's in `seq` order. An envelope seen twice (the same
 * stream and seq) counts once; any other event, a kind this version doesn't know among them, is passed over. */
export function foldWebSources(envelopes: readonly Envelope[]): WebSources {
  const byStream = new Map<StreamId, Map<Seq, Envelope>>();
  for (const e of envelopes) {
    let seen = byStream.get(e.stream);
    if (seen === undefined) byStream.set(e.stream, (seen = new Map()));
    if (!seen.has(e.seq)) seen.set(e.seq, e);
  }
  const out = new Map<StreamId, Map<ToolCallId, ToolCallWeb>>();
  for (const [stream, seen] of byStream) {
    const calls = new Map<ToolCallId, ToolCallWeb>();
    const webOf = (call: ToolCallId) => {
      let web = calls.get(call);
      if (web === undefined) calls.set(call, (web = { pages: [] }));
      return web;
    };
    for (const e of [...seen.values()].sort((a, b) => a.seq - b.seq)) {
      // read by key: an event of a kind this version doesn't know has neither
      const event = e.event as {
        web_search_recorded?: WebSearchRecorded;
        page_visited?: PageVisited;
      };
      const search = event.web_search_recorded;
      // a later search replaces the earlier one
      if (search !== undefined)
        webOf(search.tool_call).search = { query: search.query, results: search.results };
      const page = event.page_visited;
      const pages = page === undefined ? null : webOf(page.tool_call).pages;
      if (page !== undefined && pages !== null && pages.length < MAX_PAGES)
        pages.push({
          url: page.url,
          ...(page.title === undefined ? {} : { title: page.title }),
          // a way from a newer Clauseo reads as unknown, which the row says as a plain read
          via: VIAS.includes(page.via) ? page.via : "unknown",
        });
    }
    if (calls.size > 0) out.set(stream, calls);
  }
  return out;
}

/** Whether each answer's list starts open: false in the room, which shows it shut until pressed; the copy review sets
 * it so a reviewer reads the rows without a click (a press in its StrictMode render ran twice and shut it again). */
export const SourcesOpenAtFirst = createContext(false);

/** The transport the sources come from and their links open through: ThreadList's. */
export const SourcesTransport = createContext<HostTransport | null>(null);

const NONE: ReadonlyMap<ToolCallId, ToolCallWeb> = new Map();
const EMPTY: WebSources = new Map();

/** One transport's sources: every envelope it delivered, folded again as more come. */
class SourcesStore {
  #envelopes: Envelope[] = [];
  #folded: WebSources = EMPTY;
  readonly #listeners = new Set<() => void>();

  constructor(transport: HostTransport) {
    const source = transport as Partial<EnvelopeSource>;
    // the store lives as long as its transport (the WeakMap below), so it never lets go
    source.onEnvelopes?.((envelopes) => {
      this.#envelopes = [...this.#envelopes, ...envelopes];
      this.#folded = foldWebSources(this.#envelopes);
      for (const listener of this.#listeners) listener();
    });
  }

  subscribe = (listener: () => void): (() => void) => {
    this.#listeners.add(listener);
    return () => this.#listeners.delete(listener);
  };

  get folded(): WebSources {
    return this.#folded;
  }
}

const stores = new WeakMap<HostTransport, SourcesStore>();

/** The transport's sources store, made the first time it is asked for. */
export function sourcesOf(transport: HostTransport): SourcesStore {
  let store = stores.get(transport);
  if (store === undefined) stores.set(transport, (store = new SourcesStore(transport)));
  return store;
}

/** The picker of an exchange's tool calls (list.tsx): the tool call ids of its tool call entries, from an older page or
 * the window, in order, joined by line breaks so a flush that leaves them as they were renders nothing. `tools` are
 * the exchange's tool call entry ids alone, taken once per id list. */
export function toolCallsPick(
  tools: readonly EntryId[],
  paged: ReadonlyMap<EntryId, ViewEntry>,
): (v: StreamView | undefined) => string {
  return (v) =>
    tools
      .map((id) => (paged.get(id) ?? v?.entries.get(id))?.tool_call?.tool_call)
      .filter((call) => call !== undefined)
      .join("\n");
}

/** A thread's tool calls' web, by tool call id; the same empty map for a thread with none. */
export function useToolCallWeb(
  transport: HostTransport,
  stream: StreamId,
): ReadonlyMap<ToolCallId, ToolCallWeb> {
  const store = sourcesOf(transport);
  const pick = () => store.folded.get(stream) ?? NONE;
  return useSyncExternalStore(store.subscribe, pick, pick);
}

/** The schemes a source may link to: the web only, as an answer's links allow it (markdown/link.tsx) but without
 * mailto. The host logs a page's address as the agent gave it, so a data:, file: or javascript: address can come. */
const WEB_PREFIXES = LINK_PREFIXES.filter((prefix) => prefix.startsWith("http"));

/** Whether an address may sit under a link: http or https only. */
export const linkable = (url: string): boolean => {
  const lower = url.trimStart().toLowerCase();
  return WEB_PREFIXES.some((prefix) => lower.startsWith(prefix));
};
