// The matter's conversations as Versions reads them: their titles, and whether an answer runs in one, for Restore and
// Undo to wait on. list_threads gives both; it runs when Versions opens, when the matter's stream moves and when the
// host refuses an act because an answer runs. A conversation list_threads calls busy has its own stream held while it
// is, so its header says the moment it rests, even when its turn changed no file and so left the matter's stream
// still; the hold ends once it rests. Nothing here runs a timer: every read follows a host answer or a store flush.
import type { ViewStore } from "../../store/store.ts";
import { streamOf } from "../../store/view.ts";
import type { HostTransport } from "../../transport/host-transport.ts";
import type { MatterId, ThreadId, ThreadStatus, ThreadSummary } from "../../transport/protocol.ts";
import { isBusy } from "./state.ts";

export class Conversations {
  readonly #transport: HostTransport;
  readonly #store: ViewStore;
  readonly #matter: MatterId;
  readonly #changed: () => void;
  #threads: readonly ThreadSummary[] = [];
  #listing = false;
  #again = false;
  /** How many list_threads have started, and the number of the newest that came back. */
  #started = 0;
  #listed = 0;
  /** How many held conversations have come to rest. */
  #rests = 0;
  /** Each busy conversation whose stream is held, with its release. */
  readonly #holding = new Map<ThreadId, () => void>();
  /** Statuses a held conversation's header gave as it came to rest, since the last list_threads: its stream is let
   * go, and list_threads' older word must not stand. */
  readonly #rested = new Map<ThreadId, ThreadStatus>();

  constructor(transport: HostTransport, store: ViewStore, matter: MatterId, changed: () => void) {
    this.#transport = transport;
    this.#store = store;
    this.#matter = matter;
    this.#changed = changed;
  }

  get threads(): readonly ThreadSummary[] {
    return this.#threads;
  }

  /** The number the next list_threads will carry: one that comes back with it or later was asked after now. */
  get next(): number {
    return this.#started + 1;
  }

  /** The number of the newest list_threads that came back. */
  get listed(): number {
    return this.#listed;
  }

  /** How many held conversations have come to rest so far. */
  get rests(): number {
    return this.#rests;
  }

  /** A conversation's status: its header's where the store holds its stream (the newer word), else list_threads'. */
  statusOf(t: ThreadSummary): ThreadStatus {
    const h = this.#store.view(streamOf(t.thread))?.header;
    if (h !== undefined && h !== null && "header" in h && h.header !== undefined)
      return h.header.status;
    return this.#rested.get(t.thread) ?? t.status;
  }

  /** The first conversation an answer runs in, or null. */
  busy(): ThreadId | null {
    return this.#threads.find((t) => isBusy(this.statusOf(t)))?.thread ?? null;
  }

  /** list_threads, one at a time; asked again while one runs, it runs once more after. */
  list(): void {
    if (this.#listing) {
      this.#again = true;
      return;
    }
    this.#listing = true;
    this.#started += 1;
    const n = this.#started;
    this.#transport
      .query({ list_threads: { matter: this.#matter } })
      .then(
        (r) => {
          if ("threads" in r) this.#threads = r.threads;
          this.#rested.clear();
          this.#listed = n;
        },
        (error: unknown) => console.warn(`list_threads failed: ${JSON.stringify(error)}`),
      )
      .finally(() => {
        this.#listing = false;
        this.#hold();
        this.#changed();
        if (this.#again) {
          this.#again = false;
          this.list();
        }
      });
  }

  /** Holds the stream of each conversation that is busy and not yet held. */
  #hold(): void {
    for (const t of this.#threads)
      if (isBusy(this.statusOf(t)) && !this.#holding.has(t.thread))
        this.#holding.set(t.thread, this.#store.watch(streamOf(t.thread)));
  }

  /** After a store flush: lets go of each held conversation whose header says it rests. */
  read(): void {
    for (const [thread, release] of this.#holding) {
      const h = this.#store.view(streamOf(thread))?.header;
      const status = h !== undefined && h !== null && "header" in h ? h.header?.status : undefined;
      if (status === undefined || isBusy(status)) continue;
      this.#rested.set(thread, status);
      this.#holding.delete(thread);
      this.#rests += 1;
      release();
    }
  }

  /** Lets go of every held conversation. */
  stop(): void {
    for (const release of this.#holding.values()) release();
    this.#holding.clear();
  }
}
