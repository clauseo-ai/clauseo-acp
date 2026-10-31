import { expect, test, vi } from "vitest";
import { Subscriptions } from "./subscriptions.ts";
import type { BatchSubscriber } from "./host-transport.ts";
import type { HostError } from "./protocol.ts";

/** A subscriber that does nothing, or hands batches and failures to the functions given. */
const sub = (
  batch: (b: PatchBatch) => void = () => {},
  failed: (e: HostError) => void = () => {},
): BatchSubscriber => ({ batch, failed });
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
  /** Runs the deferred releases, then lets each stream's chained host calls run. */
  const settle = async () => {
    deferred.splice(0).forEach((run) => run());
    // A macrotask runs only once every microtask queued before it has, chained host calls included.
    await new Promise((done) => setTimeout(done, 0));
  };
  return { subs, calls, settle };
}

const B: PatchBatch = { stream: "s", epoch: "e", patches: [] };

test("two subscribers share one host subscription; it ends after the last leaves", async () => {
  const { subs, calls, settle } = rig();
  const a = subs.add("s", null, sub());
  const b = subs.add("s", null, sub());
  a();
  await settle();
  expect(calls).toEqual(["subscribe s null"]);
  b();
  b();
  await settle();
  expect(calls).toEqual(["subscribe s null", "unsubscribe s"]);
});

test("Strict Mode's unsubscribe then subscribe keeps one host subscription and delivers each batch once", async () => {
  const { subs, calls, settle } = rig();
  const got: PatchBatch[] = [];
  const subscriber = sub((b) => got.push(b));
  subs.add("s", null, subscriber)();
  subs.add("s", null, subscriber);
  await settle();
  subs.deliver(B);
  expect(calls).toEqual(["subscribe s null"]);
  expect(got).toEqual([B]);
});

test("resubscribe asks the host again only for a held stream; an unheld stream's batch is dropped", async () => {
  const { subs, calls, settle } = rig();
  subs.resubscribe("s", { epoch: "e", cursor: 4 });
  subs.add("s", null, sub());
  subs.resubscribe("s", { epoch: "e", cursor: 4 });
  let got = 0;
  subs.deliver({ ...B, stream: "other" });
  subs.add(
    "t",
    null,
    sub(() => (got += 1)),
  );
  subs.deliver({ ...B, stream: "t" });
  await settle();
  // Each stream's calls in order; streams are independent of each other.
  expect(calls.filter((c) => c.includes(" s "))).toEqual(["subscribe s null", "subscribe s 4"]);
  expect(calls.filter((c) => c.includes(" t "))).toEqual(["subscribe t null"]);
  expect(got).toBe(1);
});

test("a stream's host calls run in order: a subscribe waits for the unsubscribe before it to settle", async () => {
  const log: string[] = [];
  const open: (() => void)[] = [];
  const slow = (name: string) => () =>
    new Promise<void>((resolve) => {
      log.push(`start ${name}`);
      open.push(() => {
        log.push(`end ${name}`);
        resolve();
      });
    });
  const subs = new Subscriptions(
    { subscribe: (s) => slow(`subscribe ${s}`)(), unsubscribe: (s) => slow(`unsubscribe ${s}`)() },
    () => {},
    (run) => run(),
  );
  const settle = async () => {
    // A macrotask runs only once every microtask queued before it has, chained host calls included.
    await new Promise((done) => setTimeout(done, 0));
  };
  subs.add("s", null, sub())();
  subs.add("s", null, sub());
  subs.add("t", null, sub());
  await settle();
  expect(log).toEqual(["start subscribe s", "start subscribe t"]);
  open.shift()!();
  await settle();
  expect(log.slice(2)).toEqual(["end subscribe s", "start unsubscribe s"]);
  open.splice(1, 1)[0]!();
  await settle();
  expect(log.slice(4)).toEqual(["end unsubscribe s", "start subscribe s"]);
});

test("a refused subscribe reaches every subscriber as a HostError; the stream's next call still runs", async () => {
  const calls: string[] = [];
  const warnings: string[] = [];
  const failures: HostError[] = [];
  const subs = new Subscriptions(
    {
      subscribe: async () => {
        calls.push("subscribe");
        throw new Error("no");
      },
      unsubscribe: async () => {
        calls.push("unsubscribe");
        throw "gone";
      },
    },
    (problem) => warnings.push(problem),
    (run) => run(),
  );
  const release = subs.add(
    "s",
    null,
    sub(undefined, (e) => failures.push(e)),
  );
  // A macrotask runs only once every microtask queued before it has, chained host calls included.
  await new Promise((done) => setTimeout(done, 0));
  release();
  await new Promise((done) => setTimeout(done, 0));
  expect(calls).toEqual(["subscribe", "unsubscribe"]);
  expect(failures).toEqual([{ code: "internal", message: "no", retryable: false }]);
  expect(warnings).toEqual([
    'unsubscribe of s failed: {"code":"internal","message":"gone","retryable":false}',
  ]);
});

test("fail tells each subscriber of the stream once; deliver says how many got the batch", () => {
  const { subs } = rig();
  const failures: string[] = [];
  const one = sub(undefined, (e) => failures.push(`one ${e.code}`));
  subs.add("s", null, one);
  subs.add("s", null, one);
  subs.add(
    "s",
    null,
    sub(undefined, (e) => failures.push(`two ${e.code}`)),
  );
  subs.fail("s", { code: "store.io", message: "", retryable: true });
  subs.fail("nobody", { code: "store.io", message: "", retryable: true });
  expect(failures).toEqual(["one store.io", "two store.io"]);
  expect(subs.deliver(B)).toBe(2);
  expect(subs.deliver({ ...B, stream: "nobody" })).toBe(0);
});

test("the default release scheduler calls queueMicrotask unbound, as WebView2 requires", async () => {
  // WebView2 throws "Illegal invocation" when a host function runs with another object as `this`.
  const strict = vi.fn(function (this: unknown, run: () => void) {
    if (this !== undefined && this !== globalThis) throw new TypeError("Illegal invocation");
    run();
  });
  vi.stubGlobal("queueMicrotask", strict);
  try {
    const calls: string[] = [];
    const subs = new Subscriptions(
      {
        subscribe: async () => {
          calls.push("subscribe");
        },
        unsubscribe: async () => {
          calls.push("unsubscribe");
        },
      },
      () => {},
    );
    expect(() => subs.add("s", null, sub())()).not.toThrow();
    expect(strict).toHaveBeenCalledTimes(1);
  } finally {
    vi.unstubAllGlobals();
  }
  await new Promise((done) => setTimeout(done, 0));
});
