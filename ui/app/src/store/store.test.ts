import { expect, test } from "vitest";
import { FakeTransport, type FakeHost } from "../transport/fake-transport.ts";
import {
  agentText,
  append,
  batch,
  EPOCH,
  event,
  reset,
  STREAM,
  upsert,
  userMessage,
  toolCall,
} from "../testing/stream-script.ts";
import type { Batch } from "./batch.ts";
import { createViewStore } from "./store.ts";

function rig(host: FakeHost = {}) {
  const transport = new FakeTransport(host);
  const pending: (() => void)[] = [];
  const manual: Batch = (flush) => {
    pending.push(flush);
    return () => pending.splice(pending.indexOf(flush), 1);
  };
  const store = createViewStore(transport, { batch: () => manual });
  let notified = 0;
  store.subscribe(() => (notified += 1));
  const frame = () => pending.splice(0).forEach((f) => f());
  return { transport, store, frame, pending, notified: () => notified };
}

/** Lets each stream's chained host calls run. */
const settle = async () => {
  // A macrotask runs only once every microtask queued before it has, chained host calls included.
  await new Promise((done) => setTimeout(done, 0));
};

const START = event(batch(1, [reset([userMessage(5, 1, "q"), agentText(6, 1, "")])]));

test("batches wait for the frame, apply together, notify once and ack once per stream with its position", () => {
  const { transport, store, frame, pending, notified } = rig();
  store.watch(STREAM);
  transport.deliver(START);
  transport.deliver(event(batch(2, [append(1, 0, "a")])));
  transport.deliver(event(batch(3, [append(1, 1, "b")])));
  expect(pending).toHaveLength(1);
  expect(store.view(STREAM)).toBeUndefined();
  frame();
  expect(notified()).toBe(1);
  expect(store.view(STREAM)?.cursor).toBe(3);
  expect(transport.only("ack")).toEqual([
    { call: "ack", stream: STREAM, position: { epoch: EPOCH, cursor: 3 } },
  ]);
});

test("a from_len mismatch resubscribes once after the last applied cursor, until a batch applies", async () => {
  const { transport, store, frame } = rig();
  store.watch(STREAM);
  transport.deliver(START);
  frame();
  transport.deliver(event(batch(2, [append(1, 5, "x")])));
  transport.deliver(event(batch(3, [append(1, 6, "y")])));
  frame();
  transport.deliver(event(batch(2, [append(1, 7, "z")])));
  frame();
  await settle();
  expect(transport.only("subscribe").map((c) => c.after)).toEqual([
    null,
    { epoch: EPOCH, cursor: 1 },
  ]);
  transport.deliver(event(batch(2, [append(1, 0, "ok")])));
  frame();
  expect(store.view(STREAM)?.cursor).toBe(2);
  transport.deliver(event(batch(4, [upsert(toolCall(9, 1, "pending"))])));
  frame();
  await settle();
  expect(transport.only("subscribe")).toHaveLength(3);
});

test("a flush that applies only a replay still acks the stream's position, so the transport counts it processed", () => {
  const { transport, store, frame } = rig();
  store.watch(STREAM);
  transport.deliver(START);
  frame();
  transport.deliver(START);
  frame();
  expect(transport.only("ack").map((a) => a.position.cursor)).toEqual([1, 1]);
});

test("a stream nobody watches has its view dropped; watching it again starts from a Reset", async () => {
  const { transport, store, frame } = rig();
  const a = store.watch(STREAM);
  const b = store.watch(STREAM);
  transport.deliver(START);
  frame();
  a();
  b();
  await settle();
  expect(store.view(STREAM)).toBeUndefined();
  store.watch(STREAM);
  await settle();
  expect(transport.calls.filter((c) => c.call !== "ack")).toEqual([
    { call: "subscribe", stream: STREAM, after: null },
    { call: "unsubscribe", stream: STREAM },
    { call: "subscribe", stream: STREAM, after: null },
  ]);
});

test("a release and a watch again in one task (Strict Mode) keep the view and the one subscription", async () => {
  const { transport, store, frame } = rig();
  store.watch(STREAM)();
  store.watch(STREAM);
  transport.deliver(START);
  frame();
  await settle();
  expect(store.view(STREAM)?.cursor).toBe(1);
  expect(transport.only("subscribe")).toHaveLength(1);
});

test("a batch still queued when its stream is released is dropped, with no view and no ack", async () => {
  const { transport, store, frame } = rig();
  const release = store.watch(STREAM);
  transport.deliver(START);
  release();
  await settle();
  frame();
  expect(store.view(STREAM)).toBeUndefined();
  expect(transport.only("ack")).toEqual([]);
});

test("a refused subscribe and a failed stream show as the stream's error until retry asks again after the position held", async () => {
  let refuse = true;
  const { transport, store, frame, notified } = rig({
    subscribe: () => {
      if (refuse) throw { code: "store.io", message: "disk", retryable: true };
    },
  });
  store.watch(STREAM);
  await settle();
  expect(store.error(STREAM)?.code).toBe("store.io");
  expect(notified()).toBe(1);
  refuse = false;
  store.retry(STREAM);
  await settle();
  expect(store.error(STREAM)).toBeUndefined();
  transport.deliver(START);
  frame();
  // The bridge's own restart fails later: its channel message reports it.
  transport.deliver({
    stream_failed: {
      stream: STREAM,
      error: { code: "store.corrupt", message: "x", retryable: false },
    },
  });
  expect(store.error(STREAM)?.code).toBe("store.corrupt");
  store.retry(STREAM);
  await settle();
  expect(transport.only("subscribe").map((c) => c.after)).toEqual([
    null,
    null,
    { epoch: EPOCH, cursor: 1 },
  ]);
  expect(store.error(STREAM)).toBeUndefined();
});

test("a batch that applies clears the stream's error", () => {
  const { transport, store, frame } = rig();
  store.watch(STREAM);
  transport.failStream(STREAM, { code: "internal", message: "x", retryable: false });
  expect(store.error(STREAM)?.code).toBe("internal");
  transport.deliver(START);
  frame();
  expect(store.error(STREAM)).toBeUndefined();
});
