// The window's views, built from the host's patch batches (the design's stream store with the protocol's patches as
// its input). Batches wait in a queue and apply together once a frame (`frameBatch`, every 100 ms in lite), so React
// commits at most once a frame however fast the text comes. After each flush the store acknowledges, once per stream
// that advanced, with the window's frame count, which is how the bridge frees what it holds. Every channel message
// schedules a flush, so frames that deliver no batch (a stream already released, an event that is not patches) are
// acknowledged too.
import type { HostTransport } from "../transport/host-transport.ts";
import type { PatchBatch, StreamId } from "../transport/protocol.ts";
import type { Batch } from "./batch.ts";
import { applyBatch, emptyView, positionOf, type StreamView } from "./view.ts";

export type ViewStore = {
  /** Holds a stream: subscribes (ref-counted) after the position the store has for it. Returns the release. */
  watch: (stream: StreamId) => () => void;
  /** A stream's view, the same object while nothing in it changes. */
  view: (stream: StreamId) => StreamView | undefined;
  /** Called after each flush that changed a view. */
  subscribe: (listener: () => void) => () => void;
  /** Applies everything waiting, now. */
  flush: () => void;
  /** Drops anything waiting and stops any batch waiting to run. */
  dispose: () => void;
};

export type StoreOptions = {
  /** Read for each batch, so lite can switch mid-stream. */
  batch: () => Batch;
  /** Where a failed acknowledgement is reported. */
  warn?: (message: string) => void;
};

export function createViewStore(transport: HostTransport, options: StoreOptions): ViewStore {
  const warn = options.warn ?? ((m: string) => console.warn(m));
  const views = new Map<StreamId, StreamView>();
  const listenersOf = new Map<StreamId, (batch: PatchBatch) => void>();
  const watched = new Map<StreamId, number>();
  /** Streams asked for again, until a batch applies: their failing batches ask no further. */
  const resubscribing = new Set<StreamId>();
  const listeners = new Set<() => void>();
  let queue: PatchBatch[] = [];
  let cancel: (() => void) | null = null;
  let ackedFrames = 0;

  const ack = (view: StreamView, frames: number) => {
    if (view.epoch === null || view.cursor === null) return;
    transport
      .ack(view.stream, view.epoch, view.cursor, frames)
      .catch((e: unknown) => warn(`ack of ${view.stream} failed: ${JSON.stringify(e)}`));
  };

  const schedule = () => {
    cancel ??= options.batch()(flush);
  };

  const flush = () => {
    cancel = null;
    const batches = queue;
    queue = [];
    const advanced = new Set<StreamId>();
    const failed = new Set<StreamId>();
    let changed = false;
    for (const batch of batches) {
      if (failed.has(batch.stream)) continue;
      const was = views.get(batch.stream) ?? emptyView(batch.stream);
      const r = applyBatch(was, batch);
      if (r.view !== was) {
        views.set(batch.stream, r.view);
        changed = true;
      }
      if (r.advanced) {
        advanced.add(batch.stream);
        resubscribing.delete(batch.stream);
      }
      if (r.resubscribe) {
        failed.add(batch.stream);
        if (!resubscribing.has(batch.stream)) {
          resubscribing.add(batch.stream);
          transport.resubscribe(batch.stream, positionOf(r.view));
        }
      }
    }
    if (changed) for (const fn of listeners) fn();
    const frames = transport.frames();
    for (const stream of advanced) ack(views.get(stream)!, frames);
    if (advanced.size === 0 && frames > ackedFrames) {
      // Frames that moved no cursor still free the bridge's bytes. The bridge counts frames per window whatever the
      // stream, so the ack goes with any position the store holds, a watched stream's first. With no position at all
      // there is nothing to ack with; the frames wait for the next ack.
      const held = [...watched.keys(), ...views.keys()]
        .map((stream) => views.get(stream))
        .find((view) => view !== undefined && view.cursor !== null);
      if (held !== undefined) ack(held, frames);
      else return;
    }
    ackedFrames = frames;
  };

  const stopFrames = transport.onFrame(schedule);

  const listenerFor = (stream: StreamId) => {
    let fn = listenersOf.get(stream);
    if (fn === undefined) {
      fn = (batch) => {
        queue.push(batch);
        schedule();
      };
      listenersOf.set(stream, fn);
    }
    return fn;
  };

  return {
    watch: (stream) => {
      watched.set(stream, (watched.get(stream) ?? 0) + 1);
      const release = transport.subscribe(
        stream,
        positionOf(views.get(stream)),
        listenerFor(stream),
      );
      let done = false;
      return () => {
        if (done) return;
        done = true;
        const n = (watched.get(stream) ?? 1) - 1;
        if (n === 0) watched.delete(stream);
        else watched.set(stream, n);
        release();
      };
    },
    view: (stream) => views.get(stream),
    subscribe: (fn) => {
      listeners.add(fn);
      return () => listeners.delete(fn);
    },
    flush: () => {
      cancel?.();
      flush();
    },
    dispose: () => {
      stopFrames();
      cancel?.();
      cancel = null;
      queue = [];
    },
  };
}
