import { expect, test } from "vitest";
import { Subscriptions } from "./subscriptions.ts";
import type { PatchBatch } from "./protocol.ts";

function rig() {
  const calls: string[] = [];
  const deferred: (() => void)[] = [];
  const subs = new Subscriptions(
    {
      subscribe: async (s, after) => {
        calls.push(`subscribe ${s} ${after === null ? "null" : after.cursor}`);
      },
      unsubscribe: async (s) => {
        calls.push(`unsubscribe ${s}`);
      },
    },
    () => {},
    (run) => deferred.push(run),
  );
  const settle = () => deferred.splice(0).forEach((run) => run());
  return { subs, calls, settle };
}

const B: PatchBatch = { stream: "s", epoch: "e", patches: [] };

test("two listeners share one host subscription; it ends after the last leaves", () => {
  const { subs, calls, settle } = rig();
  const a = subs.add("s", null, () => {});
  const b = subs.add("s", null, () => {});
  a();
  settle();
  expect(calls).toEqual(["subscribe s null"]);
  b();
  b();
  settle();
  expect(calls).toEqual(["subscribe s null", "unsubscribe s"]);
});

test("Strict Mode's unsubscribe then subscribe keeps one host subscription and delivers each batch once", () => {
  const { subs, calls, settle } = rig();
  const got: PatchBatch[] = [];
  const listener = (b: PatchBatch) => got.push(b);
  subs.add("s", null, listener)();
  subs.add("s", null, listener);
  settle();
  subs.deliver(B);
  expect(calls).toEqual(["subscribe s null"]);
  expect(got).toEqual([B]);
});

test("resubscribe asks the host again only for a held stream; an unheld stream's batch is dropped", () => {
  const { subs, calls } = rig();
  subs.resubscribe("s", { epoch: "e", cursor: 4 });
  subs.add("s", null, () => {});
  subs.resubscribe("s", { epoch: "e", cursor: 4 });
  let got = 0;
  subs.deliver({ ...B, stream: "other" });
  subs.add("t", null, () => (got += 1));
  subs.deliver({ ...B, stream: "t" });
  expect(calls).toEqual(["subscribe s null", "subscribe s 4", "subscribe t null"]);
  expect(got).toBe(1);
});
