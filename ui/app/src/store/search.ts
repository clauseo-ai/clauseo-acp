// Find's search inside the matters (U16a, H-search 0043): the words in the field, sent to the host as they change, and
// the hits that come back. A query goes on every change of the field, with no debounce and no timer: the host
// supersedes an older search from the same page with a newer one, by its `generation`, so only the newest runs.
// The generation counts up per page (the desktop keys it by the window's label and the page's own stream number, so a
// reload counts afresh, crates/desktop/src/bridge/search.rs), which is why the counter is this module's, shared by
// every Search the page makes: the host refuses a generation lower than one it has heard from the page. An answer to
// an older generation than the newest sent, or "cancelled", is dropped. Empty words send nothing and clear the hits.
// A refused search shows nothing new: the hits stay as they were, and only its code goes to the console.
// More hits come a page at a time (`more`, as the list scrolls to its foot), through the page's `next` cursor, each
// page under its own generation, so typing meanwhile drops it.
import { toHostError, type HostTransport } from "../transport/host-transport.ts";
import type { SearchHit, SearchProgress, SearchScope } from "../transport/protocol.ts";

/** Hits a page asks for: Find draws at most this many before the list is scrolled to its foot. */
export const LIMIT = 20;

/** The page's last generation sent, across every Search it makes. */
let generation = 0;

export type SearchState = {
  /** The words asked for last; "" with nothing asked. */
  query: string;
  /** The words `hits` answer: `query` once its answer is in, the words before until then. */
  answered: string;
  hits: readonly SearchHit[];
  /** The cursor for more hits; null when there are no more. */
  next: string | null;
  /** The index is still being built: some conversations may not be found yet. Null once a page says nothing. */
  indexing: SearchProgress | null;
  /** The answer for `query` is in, or it was refused. */
  settled: boolean;
  /** A page of more hits is on its way. */
  paging: boolean;
};

export const NO_SEARCH: SearchState = {
  query: "",
  answered: "",
  hits: [],
  next: null,
  indexing: null,
  settled: true,
  paging: false,
};

export class Search {
  readonly #transport: HostTransport;
  readonly #scope: SearchScope;
  readonly #listeners = new Set<() => void>();
  #state: SearchState = NO_SEARCH;
  /** The generation whose answer is wanted; 0 wants none. */
  #wanted = 0;
  #live = false;

  constructor(transport: HostTransport, scope: SearchScope = "all") {
    this.#transport = transport;
    this.#scope = scope;
  }

  state = (): SearchState => this.#state;

  subscribe = (listener: () => void): (() => void) => {
    this.#listeners.add(listener);
    return () => this.#listeners.delete(listener);
  };

  /** Hears answers until the returned stop; an answer that lands after it is dropped. Started again (StrictMode's
   * second mount, with the words already set), it asks again for what the stop dropped: the words not yet answered,
   * or the page of more on its way. */
  start(): () => void {
    this.#live = true;
    const s = this.#state;
    if (!s.settled) this.#ask(s.query, null);
    else if (s.paging && s.next !== null) this.#ask(s.query, s.next);
    return () => {
      this.#live = false;
      this.#wanted = 0;
    };
  }

  /** The field's words changed: a new search for them, or, empty, none and no hits. The words go as typed: a trailing
   * space tells the host the last word is whole. */
  set(query: string): void {
    if (query === this.#state.query) return;
    if (query.trim() === "") {
      this.#wanted = 0;
      this.#put(NO_SEARCH);
      return;
    }
    this.#put({ ...this.#state, query, settled: false, paging: false });
    this.#ask(query, null);
  }

  /** The next page of hits for the words answered, if there is one and none is on its way. */
  more(): void {
    const s = this.#state;
    if (!s.settled || s.paging || s.next === null || s.answered !== s.query) return;
    this.#put({ ...s, paging: true });
    this.#ask(s.query, s.next);
  }

  #ask(query: string, cursor: string | null): void {
    generation += 1;
    const asked = generation;
    this.#wanted = asked;
    const request = {
      query,
      scope: this.#scope,
      include_archived: true,
      limit: LIMIT,
      generation: asked,
      ...(cursor === null ? {} : { cursor }),
    };
    this.#transport.query({ search: request }).then(
      (result) => {
        if (!this.#live || asked !== this.#wanted) return;
        const answer = "search" in result ? result.search : undefined;
        if (answer === undefined) return this.#refused("search answered something else");
        if (answer === "cancelled") return;
        const { page } = answer;
        this.#put({
          query,
          answered: query,
          hits: cursor === null ? page.hits : [...this.#state.hits, ...page.hits],
          next: page.next ?? null,
          indexing: page.indexing ?? null,
          settled: true,
          paging: false,
        });
      },
      (error: unknown) => {
        if (!this.#live || asked !== this.#wanted) return;
        this.#refused(toHostError(error).code);
      },
    );
  }

  /** A refused search: nothing new shows; its code alone goes to the console, never the words searched. */
  #refused(code: string): void {
    console.warn(`search failed: ${code}`);
    this.#put({ ...this.#state, settled: true, paging: false });
  }

  #put(state: SearchState): void {
    this.#state = state;
    for (const listener of this.#listeners) listener();
  }
}
