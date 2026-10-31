import { expect, test, vi } from "vitest";
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
  /** Runs the deferred releases, then lets each stream's chained host calls run. */
  const settle = async () => {
    deferred.splice(0).forEach((run) => run());
    // A macrotask runs only once every microtask queued before it has, chained host calls included.
    await new Promise((done) => setTimeout(done, 0));
  };
  return { subs, calls, settle };
}

const B: PatchBatch = { stream: "s", epoch: "e", patches: [] };

test("two listeners share one host subscription; it ends after the last leaves", async () => {
  const { subs, calls, settle } = rig();
  const a = subs.add("s", null, () => {});
  const b = subs.add("s", null, () => {});
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
  const listener = (b: PatchBatch) => got.push(b);
  subs.add("s", null, listener)();
  subs.add("s", null, listener);
  await settle();
  subs.deliver(B);
  expect(calls).toEqual(["subscribe s null"]);
  expect(got).toEqual([B]);
});

test("resubscribe asks the host again only for a held stream; an unheld stream's batch is dropped", async () => {
  const { subs, calls, settle } = rig();
  subs.resubscribe("s", { epoch: "e", cursor: 4 });
  subs.add("s", null, () => {});
  subs.resubscribe("s", { epoch: "e", cursor: 4 });
  let got = 0;
  subs.deliver({ ...B, stream: "other" });
  subs.add("t", null, () => (got += 1));
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
  subs.add("s", null, () => {})();
  subs.add("s", null, () => {});
  subs.add("t", null, () => {});
  await settle();
  expect(log).toEqual(["start subscribe s", "start subscribe t"]);
  open.shift()!();
  await settle();
  expect(log.slice(2)).toEqual(["end subscribe s", "start unsubscribe s"]);
  open.splice(1, 1)[0]!();
  await settle();
  expect(log.slice(4)).toEqual(["end unsubscribe s", "start subscribe s"]);
});

test("a failed host call is reported and the next call for the stream still runs", async () => {
  const calls: string[] = [];
  const errors: string[] = [];
  const subs = new Subscriptions(
    {
      subscribe: async () => {
        calls.push("subscribe");
        throw new Error("no");
      },
      unsubscribe: async () => {
        calls.push("unsubscribe");
      },
    },
    (s, e) => errors.push(`${s}: ${String(e)}`),
    (run) => run(),
  );
  subs.add("s", null, () => {})();
  // A macrotask runs only once every microtask queued before it has, chained host calls included.
  await new Promise((done) => setTimeout(done, 0));
  expect(calls).toEqual(["subscribe", "unsubscribe"]);
  expect(errors).toEqual(["s: Error: no"]);
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
    expect(() => subs.add("s", null, () => {})()).not.toThrow();
    expect(strict).toHaveBeenCalledTimes(1);
  } finally {
    vi.unstubAllGlobals();
  }
  await new Promise((done) => setTimeout(done, 0));
});
