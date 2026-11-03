// The web sources of each answer: the searches the agent ran and the pages it opened, from the domain events
// `web_search_recorded` and `page_visited` (real binding types), folded per thread and turn (foldWebSources) and kept
// per transport (sourcesOf) for sources.tsx's list under the answer.
//
// What makes it real: the desktop host sends a window no domain events. Today only FakeTransport (an EnvelopeSource)
// delivers them, so the real app has none and draws nothing. The host work that fills it is either a view entry the
// reducer makes from these events (then this reads the view instead), or an event subscription for windows that sends
// a thread's envelopes on the channel's `events` arm (then TauriTransport becomes an EnvelopeSource). Which events
// exist at all is the host's: S26 records WebFetch, Codex's open page and the searches; #275 records a page opened in
// the browser (`via: browser`).
import { createContext, useSyncExternalStore } from "react";
import type { EnvelopeSource, HostTransport } from "../../transport/host-transport.ts";
import type {
  AgentId,
  Envelope,
  PageVia,
  PageVisited,
  Seq,
  StreamId,
  TurnId,
  WebResult,
  WebSearchRecorded,
} from "../../transport/protocol.ts";

/** One source of an answer: a web search with its results, or a page the agent opened. */
export type WebSource =
  | {
      kind: "search";
      seq: Seq;
      agent: AgentId;
      query: string;
      results: readonly WebResult[];
    }
  | { kind: "page"; seq: Seq; url: string; title: string | null; via: PageVia };

/** Each thread's sources, by stream, then by turn, in `seq` order. */
export type WebSources = ReadonlyMap<StreamId, ReadonlyMap<TurnId, readonly WebSource[]>>;

const VIAS: readonly PageVia[] = ["web_fetch", "codex_open_page", "browser"];

/** The source an envelope records, with its turn; null for any other event. */
function sourceOf(e: Envelope): [TurnId, WebSource] | null {
  // read by key: an event from a newer Clauseo, of a kind this one doesn't know, has neither
  const event = e.event as { web_search_recorded?: WebSearchRecorded; page_visited?: PageVisited };
  const search = event.web_search_recorded;
  if (search !== undefined)
    return [
      search.turn,
      {
        kind: "search",
        seq: e.seq,
        agent: search.agent,
        query: search.query,
        results: search.results,
      },
    ];
  const page = event.page_visited;
  if (page !== undefined)
    return [
      page.turn,
      {
        kind: "page",
        seq: e.seq,
        url: page.url,
        title: page.title ?? null,
        // a way from a newer Clauseo reads as unknown, which the row says as a plain read
        via: VIAS.includes(page.via) ? page.via : "unknown",
      },
    ];
  return null;
}

/** The sources in `envelopes`, per thread and turn, each turn's in `seq` order. An envelope seen twice (the same
 * stream and seq) counts once; any other event is passed over. */
export function foldWebSources(envelopes: readonly Envelope[]): WebSources {
  const byStream = new Map<StreamId, Map<Seq, Envelope>>();
  for (const e of envelopes) {
    let seen = byStream.get(e.stream);
    if (seen === undefined) byStream.set(e.stream, (seen = new Map()));
    if (!seen.has(e.seq)) seen.set(e.seq, e);
  }
  const out = new Map<StreamId, Map<TurnId, WebSource[]>>();
  for (const [stream, seen] of byStream) {
    const turns = new Map<TurnId, WebSource[]>();
    for (const e of [...seen.values()].sort((a, b) => a.seq - b.seq)) {
      const found = sourceOf(e);
      if (found === null) continue;
      const [turn, source] = found;
      const list = turns.get(turn);
      if (list === undefined) turns.set(turn, [source]);
      else list.push(source);
    }
    if (turns.size > 0) out.set(stream, turns);
  }
  return out;
}

/** The transport the sources come from and their links open through: ThreadList's. */
export const SourcesTransport = createContext<HostTransport | null>(null);

const NONE: readonly WebSource[] = [];
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

/** One turn's sources, in order; the same empty list for a turn with none. */
export function useWebSources(
  transport: HostTransport,
  stream: StreamId,
  turn: TurnId | undefined,
): readonly WebSource[] {
  const store = sourcesOf(transport);
  const pick = () => (turn === undefined ? NONE : (store.folded.get(stream)?.get(turn) ?? NONE));
  return useSyncExternalStore(store.subscribe, pick, pick);
}
