import { expect, test } from "vitest";
import { FakeTransport } from "../transport/fake-transport.ts";
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

function rig() {
  const transport = new FakeTransport();
  const pending: (() => void)[] = [];
  const manual: Batch = (flush) => {
    pending.push(flush);
    return () => pending.splice(pending.indexOf(flush), 1);
  };
  const store = createViewStore(transport, { batch: () => manual, warn: () => {} });
  let notified = 0;
  store.subscribe(() => (notified += 1));
  const frame = () => pending.splice(0).forEach((f) => f());
  return { transport, store, frame, pending, notified: () => notified };
}

const START = event(batch(1, [reset([userMessage(5, 1, "q"), agentText(6, 1, "")])]));

test("batches wait for the frame, apply together, notify once and ack once per stream with the frame count", () => {
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
    { call: "ack", stream: STREAM, epoch: EPOCH, cursor: 3, frames: 3 },
  ]);
});

test("a from_len mismatch resubscribes once after the last applied cursor, until a batch applies", () => {
  const { transport, store, frame } = rig();
  store.watch(STREAM);
  transport.deliver(START);
  frame();
  transport.deliver(event(batch(2, [append(1, 5, "x")])));
  transport.deliver(event(batch(3, [append(1, 6, "y")])));
  frame();
  transport.deliver(event(batch(2, [append(1, 7, "z")])));
  frame();
  expect(transport.only("subscribe").map((c) => c.after)).toEqual([
    null,
    { epoch: EPOCH, cursor: 1 },
  ]);
  transport.deliver(event(batch(2, [append(1, 0, "ok")])));
  frame();
  expect(store.view(STREAM)?.cursor).toBe(2);
  transport.deliver(event(batch(4, [upsert(toolCall(9, 1, "pending"))])));
  frame();
  expect(transport.only("subscribe")).toHaveLength(3);
});

test("frames that move no cursor are still acknowledged, so the bridge can free them", () => {
  const { transport, store, frame } = rig();
  store.watch(STREAM);
  transport.deliver(START);
  frame();
  transport.deliver(START);
  frame();
  expect(transport.only("ack").map((a) => [a.cursor, a.frames])).toEqual([
    [1, 1],
    [1, 2],
  ]);
});

test("watching twice subscribes once; a stream watched again starts after the position held", async () => {
  const { transport, store, frame } = rig();
  const a = store.watch(STREAM);
  const b = store.watch(STREAM);
  transport.deliver(START);
  frame();
  a();
  b();
  await Promise.resolve();
  store.watch(STREAM);
  expect(transport.calls.filter((c) => c.call !== "ack")).toEqual([
    { call: "subscribe", stream: STREAM, after: null },
    { call: "unsubscribe", stream: STREAM },
    { call: "subscribe", stream: STREAM, after: { epoch: EPOCH, cursor: 1 } },
  ]);
});
