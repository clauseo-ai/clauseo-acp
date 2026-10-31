// Stream subscriptions, ref-counted: the host holds one subscription per stream however many subscribers there are.
// The last subscriber leaving releases it a microtask later, so React Strict Mode's unmount-and-mount of an effect
// keeps the one host subscription and each patch reaches a subscriber once.
//
// A stream's host calls run one after another: each starts when the one before it settles. Tauri runs async commands
// concurrently, so an unsubscribe followed by a subscribe (leaving a thread and coming back) could otherwise reach the
// bridge reversed and leave the stream unfollowed while the window thinks it holds it.
//
// A subscribe the host refuses, and a stream the host reports failed, reach every subscriber of the stream as
// `failed`, so the window can show the error by its code and offer to subscribe again.
import { toHostError, type BatchSubscriber } from "./host-transport.ts";
import type { HostError, PatchBatch, StreamId, ViewPosition } from "./protocol.ts";

/** The host calls the subscriptions make. */
export type SubscriptionHost = {
  subscribe(stream: StreamId, after: ViewPosition | null): Promise<void>;
  unsubscribe(stream: StreamId): Promise<void>;
};

type Entry = { count: number; subscribers: Map<BatchSubscriber, number> };

export class Subscriptions {
  readonly #streams = new Map<StreamId, Entry>();
  readonly #host: SubscriptionHost;
  readonly #defer: (run: () => void) => void;
  readonly #warn: (message: string) => void;
  /** Each stream's last host call, which the next one waits for. */
  readonly #chains = new Map<StreamId, Promise<void>>();

  constructor(
    host: SubscriptionHost,
    warn: (message: string) => void,
    // Wrapped, never passed bare: stored as a field and called as `this.#defer`, a host function gets the wrong
    // `this`, and WebView2 throws "Illegal invocation".
    defer: (run: () => void) => void = (run) => queueMicrotask(run),
  ) {
    this.#host = host;
    this.#warn = warn;
    this.#defer = defer;
  }

  /** Runs a host call for `stream` after the stream's previous one settles; at once when none is pending. */
  #run(stream: StreamId, call: () => Promise<void>, onError: (error: HostError) => void): void {
    const prev = this.#chains.get(stream);
    const started =
      prev === undefined ? new Promise<void>((done) => done(call())) : prev.then(call);
    const next = started.catch((e: unknown) => onError(toHostError(e)));
    this.#chains.set(stream, next);
    void next.then(() => {
      if (this.#chains.get(stream) === next) this.#chains.delete(stream);
    });
  }

  #subscribe(stream: StreamId, after: ViewPosition | null): void {
    this.#run(
      stream,
      () => this.#host.subscribe(stream, after),
      (error) => this.fail(stream, error),
    );
  }

  add(stream: StreamId, after: ViewPosition | null, subscriber: BatchSubscriber): () => void {
    let entry = this.#streams.get(stream);
    if (entry === undefined) {
      entry = { count: 0, subscribers: new Map() };
      this.#streams.set(stream, entry);
      this.#subscribe(stream, after);
    }
    entry.count += 1;
    entry.subscribers.set(subscriber, (entry.subscribers.get(subscriber) ?? 0) + 1);
    const held = entry;
    let done = false;
    return () => {
      if (done) return;
      done = true;
      held.count -= 1;
      const n = (held.subscribers.get(subscriber) ?? 1) - 1;
      if (n === 0) held.subscribers.delete(subscriber);
      else held.subscribers.set(subscriber, n);
      if (held.count > 0) return;
      this.#defer(() => {
        if (held.count > 0 || this.#streams.get(stream) !== held) return;
        this.#streams.delete(stream);
        this.#run(
          stream,
          () => this.#host.unsubscribe(stream),
          (error) => this.#warn(`unsubscribe of ${stream} failed: ${JSON.stringify(error)}`),
        );
      });
    };
  }

  resubscribe(stream: StreamId, after: ViewPosition | null): void {
    if (!this.#streams.has(stream)) return;
    this.#subscribe(stream, after);
  }

  /** Hands a batch to each distinct subscriber of its stream once. Returns how many got it: 0 for a stream nobody
   * holds, which drops it. */
  deliver(batch: PatchBatch): number {
    const entry = this.#streams.get(batch.stream);
    if (entry === undefined) return 0;
    for (const subscriber of entry.subscribers.keys()) subscriber.batch(batch);
    return entry.subscribers.size;
  }

  /** Tells each subscriber of `stream` that it failed. A stream nobody holds has nobody to tell. */
  fail(stream: StreamId, error: HostError): void {
    const entry = this.#streams.get(stream);
    if (entry === undefined) return;
    for (const subscriber of entry.subscribers.keys()) subscriber.failed(error);
  }
}
