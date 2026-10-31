// The window's views, built from the host's patch batches (the design's stream store with the protocol's patches as
// its input). Batches wait in a queue and apply together once a frame (`frameBatch`, every 100 ms in lite), so React
// commits at most once a frame however fast the text comes. After each flush the store acks, once per stream it
// applied a batch of, the position it holds. A stream nobody watches any more has its view dropped, so a thread the
// lawyer left does not stay in memory. A stream that fails (the host refused the subscribe, or reported its restart
// failed) holds the error until `retry` asks for it again after the last applied position.
import type { BatchSubscriber, HostTransport } from "../transport/host-transport.ts";
import type { HostError, StreamId } from "../transport/protocol.ts";
import type { Batch } from "./batch.ts";
import { applyBatch, newView, positionOf, type LiveView, type StreamView } from "./view.ts";
import type { PatchBatch } from "../transport/protocol.ts";

export type ViewStore = {
  /** Holds a stream: subscribes (ref-counted) after the position the store has for it. Returns the release. */
  watch: (stream: StreamId) => () => void;
  /** A stream's view while it is watched. Changed in place by each flush; read one part of it at a time. */
  view: (stream: StreamId) => StreamView | undefined;
  /** Why a watched stream stopped, until `retry` or a batch that applies. */
  error: (stream: StreamId) => HostError | undefined;
  /** Asks the host for a failed stream again, after the last position applied. */
  retry: (stream: StreamId) => void;
  /** Called after each flush that changed a view, and when a stream's error comes or goes. */
  subscribe: (listener: () => void) => () => void;
  /** Applies everything waiting, now. */
  flush: () => void;
  /** Drops anything waiting and stops any batch waiting to run. */
  dispose: () => void;
};

export type StoreOptions = {
  /** Read for each batch, so lite can switch mid-stream. */
  batch: () => Batch;
  /** Runs `run` once the current task's work is done: when a released stream's view is dropped, so a release and an
   * immediate watch again (Strict Mode) keep it. */
  defer?: (run: () => void) => void;
};

export function createViewStore(transport: HostTransport, options: StoreOptions): ViewStore {
  const defer = options.defer ?? ((run: () => void) => queueMicrotask(run));
  const views = new Map<StreamId, LiveView>();
  const errors = new Map<StreamId, HostError>();
  const subscribers = new Map<StreamId, BatchSubscriber>();
  const watched = new Map<StreamId, number>();
  /** Streams asked for again, until a batch applies: their failing batches ask no further. */
  const resubscribing = new Set<StreamId>();
  const listeners = new Set<() => void>();
  let queue: PatchBatch[] = [];
  let cancel: (() => void) | null = null;

  const notify = () => {
    for (const fn of listeners) fn();
  };

  const flush = () => {
    cancel = null;
    const batches = queue;
    queue = [];
    const applied = new Set<StreamId>();
    const failed = new Set<StreamId>();
    let changed = false;
    for (const batch of batches) {
      const stream = batch.stream;
      if (!watched.has(stream) || failed.has(stream)) continue;
      let view = views.get(stream);
      if (view === undefined) {
        view = newView(stream);
        views.set(stream, view);
      }
      const r = applyBatch(view, batch);
      applied.add(stream);
      if (r.changed) {
        changed = true;
        resubscribing.delete(stream);
        if (errors.delete(stream)) changed = true;
      }
      if (r.resubscribe) {
        failed.add(stream);
        if (!resubscribing.has(stream)) {
          resubscribing.add(stream);
          transport.resubscribe(stream, positionOf(view));
        }
      }
    }
    if (changed) notify();
    for (const stream of applied) {
      const position = positionOf(views.get(stream));
      if (position !== null) transport.ack(stream, position);
    }
  };

  const subscriberFor = (stream: StreamId): BatchSubscriber => {
    let subscriber = subscribers.get(stream);
    if (subscriber === undefined) {
      subscriber = {
        batch: (batch) => {
          queue.push(batch);
          cancel ??= options.batch()(flush);
        },
        failed: (error) => {
          if (!watched.has(stream)) return;
          errors.set(stream, error);
          notify();
        },
      };
      subscribers.set(stream, subscriber);
    }
    return subscriber;
  };

  return {
    watch: (stream) => {
      watched.set(stream, (watched.get(stream) ?? 0) + 1);
      const release = transport.subscribe(
        stream,
        positionOf(views.get(stream)),
        subscriberFor(stream),
      );
      let done = false;
      return () => {
        if (done) return;
        done = true;
        const n = (watched.get(stream) ?? 1) - 1;
        if (n === 0) watched.delete(stream);
        else watched.set(stream, n);
        release();
        defer(() => {
          if (watched.has(stream)) return;
          views.delete(stream);
          errors.delete(stream);
          resubscribing.delete(stream);
        });
      };
    },
    view: (stream) => views.get(stream),
    error: (stream) => errors.get(stream),
    retry: (stream) => {
      if (!watched.has(stream)) return;
      resubscribing.delete(stream);
      if (errors.delete(stream)) notify();
      transport.resubscribe(stream, positionOf(views.get(stream)));
    },
    subscribe: (fn) => {
      listeners.add(fn);
      return () => listeners.delete(fn);
    },
    flush: () => {
      cancel?.();
      flush();
    },
    dispose: () => {
      cancel?.();
      cancel = null;
      queue = [];
    },
  };
}
