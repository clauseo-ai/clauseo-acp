// Every matter's threads, for the rail's Needs you and Recent: a small store the rail reads (useSyncExternalStore),
// built once per room. Conversations (conversations.ts) holds only the matters this room has visited; this holds them
// all, as H4's app feed gives them. The host sends each matter's whole list (`all: true`) as the first frames of every
// app subscription, and the catch-up of one that lagged (crates/host/src/actor/app_lists.rs, `current`), then only
// the rows that change, so nothing is read here: no `list_threads` per matter at start. A matter taken off the list
// takes its threads with it (`matters_removed`, or `matters_changed.removed` on the generated wire). Without the app
// part (the desktop until its matters and app parts are wired) the store stays empty. Each row is kept as the bindings
// carry it: status (archived included), seen, last_stop, last_activity, title.
import type { MatterId, ThreadId, ThreadSummary, Timestamp } from "../transport/protocol.ts";
import type { AppEvent, NextHost } from "./host-next.ts";

/** Each matter's threads, as the host last gave them, and when the view was made. A matter's list keeps its identity
 * until it changes, and so does the map until any list does. */
export type AllThreadsView = {
  lists: ReadonlyMap<MatterId, readonly ThreadSummary[]>;
  /** When this view was made: Recent's Today and the rows' clock times are taken from it, so nothing ticks at rest. */
  now: Timestamp;
};

export class AllThreads {
  readonly #next: NextHost;
  readonly #by = new Map<MatterId, Map<ThreadId, ThreadSummary>>();
  readonly #listeners = new Set<() => void>();
  readonly #clock: () => Timestamp;
  #view: AllThreadsView;
  #live = false;

  constructor(next: NextHost, clock: () => Timestamp = () => Date.now()) {
    this.#next = next;
    this.#clock = clock;
    this.#view = { lists: new Map(), now: clock() };
  }

  view = (): AllThreadsView => this.#view;

  subscribe = (listener: () => void): (() => void) => {
    this.#listeners.add(listener);
    return () => this.#listeners.delete(listener);
  };

  /** Hears the app feed; returns the stop. */
  start(): () => void {
    const app = this.#next.app;
    if (app === null) return () => {};
    this.#live = true;
    const stop = app.subscribeApp((event) => this.#hear(event));
    return () => {
      this.#live = false;
      stop();
    };
  }

  #hear(event: AppEvent): void {
    if (!this.#live) return;
    const changed = new Set<MatterId>();
    if ("threads_changed" in event) {
      const change = event.threads_changed;
      const was = this.#by.get(change.matter);
      const threads = change.all || was === undefined ? new Map<ThreadId, ThreadSummary>() : was;
      for (const t of change.threads) threads.set(t.thread, t);
      for (const gone of change.removed) threads.delete(gone);
      this.#by.set(change.matter, threads);
      changed.add(change.matter);
    }
    const removed = "matters_changed" in event ? event.matters_changed.removed : [];
    for (const matter of removed) if (this.#by.delete(matter)) changed.add(matter);
    if (changed.size === 0) return;
    const lists = new Map(this.#view.lists);
    for (const matter of changed) {
      const threads = this.#by.get(matter);
      if (threads === undefined) lists.delete(matter);
      else lists.set(matter, [...threads.values()]);
    }
    this.#view = { lists, now: this.#clock() };
    for (const listener of this.#listeners) listener();
  }
}
