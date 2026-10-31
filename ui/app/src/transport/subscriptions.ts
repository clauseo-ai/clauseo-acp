// Stream subscriptions, ref-counted: the host holds one subscription per stream however many listeners there are.
// The last listener leaving releases it a microtask later, so React Strict Mode's unmount-and-mount of an effect keeps
// the one host subscription and each patch reaches a listener once.
//
// A stream's host calls run one after another: each starts when the one before it settles. Tauri runs async commands
// concurrently, so an unsubscribe followed by a subscribe (leaving a thread and coming back) could otherwise reach the
// bridge reversed and leave the stream unfollowed while the window thinks it holds it.
import type { BatchListener } from "./host-transport.ts";
import type { PatchBatch, StreamId, ViewPosition } from "./protocol.ts";

/** The host calls the subscriptions make. */
export type SubscriptionHost = {
  subscribe(stream: StreamId, after: ViewPosition | null): Promise<void>;
  unsubscribe(stream: StreamId): Promise<void>;
};

type Entry = { count: number; listeners: Map<BatchListener, number> };

export class Subscriptions {
  readonly #streams = new Map<StreamId, Entry>();
  readonly #host: SubscriptionHost;
  readonly #defer: (run: () => void) => void;
  readonly #onError: (stream: StreamId, error: unknown) => void;
  /** Each stream's last host call, which the next one waits for. */
  readonly #chains = new Map<StreamId, Promise<void>>();

  constructor(
    host: SubscriptionHost,
    onError: (stream: StreamId, error: unknown) => void,
    defer: (run: () => void) => void = queueMicrotask,
  ) {
    this.#host = host;
    this.#onError = onError;
    this.#defer = defer;
  }

  /** Runs a host call for `stream` after the stream's previous one settles; at once when none is pending. */
  #run(stream: StreamId, call: () => Promise<void>): void {
    const prev = this.#chains.get(stream);
    const started =
      prev === undefined ? new Promise<void>((done) => done(call())) : prev.then(call);
    const next = started.catch((e: unknown) => this.#onError(stream, e));
    this.#chains.set(stream, next);
    void next.then(() => {
      if (this.#chains.get(stream) === next) this.#chains.delete(stream);
    });
  }

  add(stream: StreamId, after: ViewPosition | null, listener: BatchListener): () => void {
    let entry = this.#streams.get(stream);
    if (entry === undefined) {
      entry = { count: 0, listeners: new Map() };
      this.#streams.set(stream, entry);
      this.#run(stream, () => this.#host.subscribe(stream, after));
    }
    entry.count += 1;
    entry.listeners.set(listener, (entry.listeners.get(listener) ?? 0) + 1);
    const held = entry;
    let done = false;
    return () => {
      if (done) return;
      done = true;
      held.count -= 1;
      const n = (held.listeners.get(listener) ?? 1) - 1;
      if (n === 0) held.listeners.delete(listener);
      else held.listeners.set(listener, n);
      if (held.count > 0) return;
      this.#defer(() => {
        if (held.count > 0 || this.#streams.get(stream) !== held) return;
        this.#streams.delete(stream);
        this.#run(stream, () => this.#host.unsubscribe(stream));
      });
    };
  }

  resubscribe(stream: StreamId, after: ViewPosition | null): void {
    if (!this.#streams.has(stream)) return;
    this.#run(stream, () => this.#host.subscribe(stream, after));
  }

  /** Hands a batch to each distinct listener of its stream once. A stream nobody holds drops it. */
  deliver(batch: PatchBatch): void {
    const entry = this.#streams.get(batch.stream);
    if (entry === undefined) return;
    for (const listener of entry.listeners.keys()) listener(batch);
  }

  /** Every stream held, for a page that opens its channel again. */
  streams(): StreamId[] {
    return [...this.#streams.keys()];
  }
}
