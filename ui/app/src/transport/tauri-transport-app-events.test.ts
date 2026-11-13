import { afterEach, describe, expect, test, vi } from "vitest";

// The app-events list against the transport: every key it names reaches the app listeners, and no other HostEvent key
// does. The Tauri core module is replaced as in tauri-transport.test.ts: `invoke` answers from `answers`, and the
// channel a transport opens is kept so a test can play messages into it.
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

const { TauriTransport, APP_EVENTS, OTHER_EVENTS } = await import("./tauri-transport.ts");

afterEach(() => {
  answers.clear();
  opened.length = 0;
  invoked.length = 0;
});

/** Lets every queued microtask run, the transport's chained calls and its ack included. */
const settle = () => new Promise((done) => setTimeout(done, 0));

/** A transport whose animation frames run when the test says, with its channel open and stream `a` subscribed. */
async function rig() {
  const frames: (() => void)[] = [];
  const t = new TauriTransport({ warn: () => {}, nextFrame: (run) => frames.push(run) });
  const got: string[] = [];
  t.subscribe("a", null, {
    batch: () => got.push("batch"),
    failed: (e) => got.push(e.code),
  });
  await settle();
  const send = (message: unknown) => opened.at(-1)!.onmessage(JSON.stringify(message));
  const frame = async () => {
    frames.splice(0).forEach((run) => run());
    await settle();
  };
  const acks = () => invoked.filter((i) => i.command === "ack").map((i) => i.args);
  return { t, got, send, frame, acks };
}

describe("TauriTransport's app events", () => {
  test("every APP_EVENTS key reaches an app listener, no stream subscriber, and is acked on the next frame", async () => {
    const { t, got, send, frame, acks } = await rig();
    const heard: unknown[] = [];
    await t.onAppEvent((e) => heard.push(e)).ready;
    for (const key of APP_EVENTS) send({ [key]: {} });
    expect(heard).toEqual(APP_EVENTS.map((key) => ({ [key]: {} })));
    expect(got).toEqual([]);
    await frame();
    expect(acks()).toEqual([{ positions: [], messages: APP_EVENTS.length }]);
  });

  test("the OTHER_EVENTS keys that no branch hands on reach no app listener", async () => {
    const { t, send, frame, acks } = await rig();
    const heard: unknown[] = [];
    await t.onAppEvent((e) => heard.push(e)).ready;
    const unheard = OTHER_EVENTS.filter(
      (key) => key !== "patches" && key !== "limits_view_changed",
    );
    for (const key of unheard) send({ [key]: {} });
    expect(heard).toEqual([]);
    await frame();
    expect(acks()).toEqual([{ positions: [], messages: unheard.length }]);
  });
});
