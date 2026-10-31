import { afterEach, describe, expect, test, vi } from "vitest";

// Tauri's core module, replaced: `invoke` answers per command from `answers`, and the channel a transport opens is
// kept so a test can play messages into it.
const answers = new Map<string, (args: unknown) => unknown>();
const opened: { onmessage: (m: unknown) => void }[] = [];
const invoked: { command: string; args: unknown }[] = [];
vi.mock("@tauri-apps/api/core", () => ({
  isTauri: () => false,
  invoke: async (command: string, args: unknown) => {
    invoked.push({ command, args });
    const answer = answers.get(command);
    if (answer === undefined) return null;
    return answer(args);
  },
  Channel: class {
    onmessage: (m: unknown) => void;
    constructor(onmessage: (m: unknown) => void) {
      this.onmessage = onmessage;
      opened.push(this);
    }
  },
}));

const { TauriTransport, pageTransport } = await import("./tauri-transport.ts");

afterEach(() => {
  answers.clear();
  opened.length = 0;
  invoked.length = 0;
});

/** Lets every queued microtask run, the transport's chained calls and its ack included. */
const settle = () => new Promise((done) => setTimeout(done, 0));

/** A transport whose animation frames run when the test says, with its channel open and `streams` subscribed. */
async function rig(...streams: string[]) {
  const frames: (() => void)[] = [];
  const t = new TauriTransport({ warn: () => {}, nextFrame: (run) => frames.push(run) });
  const got: { stream: string; kind: string }[] = [];
  for (const stream of streams) {
    t.subscribe(stream, null, {
      batch: (b) => got.push({ stream: b.stream, kind: "batch" }),
      failed: (e) => got.push({ stream, kind: e.code }),
    });
  }
  await settle();
  const send = (message: unknown) => opened.at(-1)!.onmessage(JSON.stringify(message));
  const frame = async () => {
    frames.splice(0).forEach((run) => run());
    await settle();
  };
  const acks = () => invoked.filter((i) => i.command === "ack").map((i) => i.args);
  return { t, got, send, frame, acks };
}

const patches = (stream: string) => ({ patches: { stream, epoch: "e", patches: [] } });

describe("TauriTransport", () => {
  test("a host error rejects as itself", async () => {
    const error = { code: "thread.turn_running", message: "busy", retryable: true };
    answers.set("dispatch", () => Promise.reject(error));
    const t = new TauriTransport({ warn: () => {} });
    await expect(
      t.dispatch({ command_id: "c", command: { cancel: { thread: "x" } } }),
    ).rejects.toEqual(error);
  });

  test("a Tauri string rejection and a thrown Error each reject as internal", async () => {
    answers.set("query", () => Promise.reject("command query not allowed"));
    answers.set("pick_folder", () => {
      throw new Error("ipc down");
    });
    const t = new TauriTransport({ warn: () => {} });
    await expect(t.query("list_matters")).rejects.toEqual({
      code: "internal",
      message: "command query not allowed",
      retryable: false,
    });
    await expect(t.pickFolder()).rejects.toEqual({
      code: "internal",
      message: "ipc down",
      retryable: false,
    });
  });

  test("one channel per transport; batches reach the stream's subscriber", async () => {
    const { got, send } = await rig("thread:x");
    expect(invoked.filter((i) => i.command === "subscribe").map((i) => i.args)).toEqual([
      { stream: "thread:x", after: null },
    ]);
    send(patches("thread:x"));
    expect(opened).toHaveLength(1);
    expect(got).toEqual([{ stream: "thread:x", kind: "batch" }]);
  });

  test("the store's acks of one flush and the message count go to the bridge as one call", async () => {
    const { t, send, acks } = await rig("a", "b");
    send(patches("a"));
    send(patches("b"));
    t.ack("a", { epoch: "e", cursor: 4 });
    t.ack("b", { epoch: "e", cursor: 9 });
    await settle();
    expect(acks()).toEqual([
      {
        positions: [
          { stream: "a", epoch: "e", cursor: 4 },
          { stream: "b", epoch: "e", cursor: 9 },
        ],
        messages: 2,
      },
    ]);
  });

  test("an ack names each stream once: the latest position the store gave for it", async () => {
    const { t, send, acks } = await rig("a");
    send(patches("a"));
    t.ack("a", { epoch: "e", cursor: 4 });
    t.ack("a", { epoch: "e", cursor: 5 });
    await settle();
    expect(acks()).toEqual([{ positions: [{ stream: "a", epoch: "e", cursor: 5 }], messages: 1 }]);
  });

  test("a stream_failed message counts as processed and is acked on the next frame", async () => {
    const { send, frame, acks } = await rig("a");
    send({
      stream_failed: { stream: "a", error: { code: "store.io", message: "x", retryable: true } },
    });
    await frame();
    expect(acks()).toEqual([{ positions: [], messages: 1 }]);
  });

  test("messages that carried nothing for a subscriber are acked on the next frame, with no positions", async () => {
    const { send, frame, acks } = await rig("a");
    send(patches("nobody"));
    send({ account_limits_changed: {} });
    await settle();
    expect(acks()).toEqual([]);
    await frame();
    expect(acks()).toEqual([{ positions: [], messages: 2 }]);
    await frame();
    expect(acks()).toHaveLength(1);
  });

  test("a message that handed over a batch counts only once the store acks; the ones before it count at once", async () => {
    const { t, send, frame, acks } = await rig("a");
    send({ account_limits_changed: {} });
    send(patches("a"));
    send({ account_limits_changed: {} });
    await frame();
    expect(acks()).toEqual([{ positions: [], messages: 1 }]);
    t.ack("a", { epoch: "e", cursor: 1 });
    await settle();
    expect(acks().at(-1)).toEqual({
      positions: [{ stream: "a", epoch: "e", cursor: 1 }],
      messages: 3,
    });
  });

  test("pieces of an event still arriving are not counted", async () => {
    const { send, frame, acks } = await rig("a");
    const text = JSON.stringify(patches("nobody"));
    send({ fragment: { id: 1, index: 0, count: 2, text: text.slice(0, 10) } });
    await frame();
    expect(acks()).toEqual([]);
    send({ fragment: { id: 1, index: 1, count: 2, text: text.slice(10) } });
    await frame();
    expect(acks()).toEqual([{ positions: [], messages: 2 }]);
  });

  test("the bridge's stream_failed message reaches the stream's subscriber as a failure", async () => {
    const { got, send } = await rig("a");
    send({
      stream_failed: { stream: "a", error: { code: "store.io", message: "x", retryable: true } },
    });
    expect(got).toEqual([{ stream: "a", kind: "store.io" }]);
  });

  test("a failed open_stream fails the stream's subscribe; a resubscribe opens the channel again", async () => {
    let refuse = true;
    answers.set("open_stream", () => (refuse ? Promise.reject("no channel") : null));
    const { t, got } = await rig("a");
    expect(got).toEqual([{ stream: "a", kind: "internal" }]);
    refuse = false;
    t.resubscribe("a", null);
    await settle();
    expect(opened).toHaveLength(2);
    expect(invoked.filter((i) => i.command === "subscribe")).toHaveLength(1);
  });

  test("onDispatch hears each command as it is issued, before the host call, until unsubscribed", async () => {
    const t = new TauriTransport({ warn: () => {} });
    const heard: string[] = [];
    const off = t.onDispatch((c) =>
      heard.push(
        `${c.command_id} before ${invoked.filter((i) => i.command === "dispatch").length} calls`,
      ),
    );
    await t.dispatch({ command_id: "a", command: { cancel: { thread: "x" } } });
    off();
    await t.dispatch({ command_id: "b", command: { cancel: { thread: "x" } } });
    expect(heard).toEqual(["a before 0 calls"]);
    expect(invoked.filter((i) => i.command === "dispatch")).toHaveLength(2);
  });

  test("connected resolves once open_stream answered", async () => {
    const t = new TauriTransport({ warn: () => {} });
    await expect(t.connected()).resolves.toBeUndefined();
    expect(invoked.some((i) => i.command === "open_stream")).toBe(true);
  });

  test("outside Tauri the page transport reaches nothing", async () => {
    const t = pageTransport();
    expect(t.kind).toBe("null");
    expect(pageTransport()).toBe(t);
    await expect(t.query("list_matters")).rejects.toMatchObject({ code: "internal" });
  });
});
