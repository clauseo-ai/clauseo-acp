// Stream subscriptions, ref-counted: the host holds one subscription per stream however many listeners there are.
// The last listener leaving releases it a microtask later, so React Strict Mode's unmount-and-mount of an effect keeps
// the one host subscription and each patch reaches a listener once.
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

  constructor(
    host: SubscriptionHost,
    onError: (stream: StreamId, error: unknown) => void,
    defer: (run: () => void) => void = queueMicrotask,
  ) {
    this.#host = host;
    this.#onError = onError;
    this.#defer = defer;
  }

  add(stream: StreamId, after: ViewPosition | null, listener: BatchListener): () => void {
    let entry = this.#streams.get(stream);
    if (entry === undefined) {
      entry = { count: 0, listeners: new Map() };
      this.#streams.set(stream, entry);
      this.#host.subscribe(stream, after).catch((e: unknown) => this.#onError(stream, e));
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
        this.#host.unsubscribe(stream).catch((e: unknown) => this.#onError(stream, e));
      });
    };
  }

  resubscribe(stream: StreamId, after: ViewPosition | null): void {
    if (!this.#streams.has(stream)) return;
    this.#host.subscribe(stream, after).catch((e: unknown) => this.#onError(stream, e));
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
