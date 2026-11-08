// The desktop transport's share of the IPC's six requests (lane.ts): reads, subscriptions and counts the page makes on
// its own hold at most BACKGROUND_SLOTS, and acks at most one, so a lawyer's action goes at once however long the
// desktop takes over them. Apart from tauri-transport.test.ts to keep that file under the size limit.
import { afterEach, expect, test, vi } from "vitest";

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

const { TauriTransport, BACKGROUND_SLOTS } = await import("./tauri-transport.ts");

afterEach(() => {
  answers.clear();
  opened.length = 0;
  invoked.length = 0;
});

const settle = () => new Promise((done) => setTimeout(done, 0));
const sent = (command: string) => invoked.filter((i) => i.command === command).length;
/** Answers that wait until `release`. */
function hold() {
  const waiting: (() => void)[] = [];
  return {
    answer: () => new Promise<unknown>((done) => waiting.push(() => done(null))),
    release: () => waiting.splice(0).forEach((done) => done()),
  };
}

test("held reads take at most their share of the IPC: the pick and a lawyer's dispatch go at once", async () => {
  const slow = hold();
  answers.set("query", slow.answer);
  answers.set("count_ui", slow.answer);
  answers.set("pick_folder_or_choose", () => ({ kind: "cancelled" }));
  const t = new TauriTransport({ warn: () => {} });
  await t.connected();
  const reads = [
    t.query("list_matters"),
    t.query("get_settings"),
    t.query("list_agents"),
    t.query("get_update"),
    t.countUi({ onboarding_step: { step: "welcome_shown" } } as never),
    t.dispatch({ command_id: "r", command: { refresh_limits: { why: "shown" } } } as never),
  ];
  await settle();
  expect(BACKGROUND_SLOTS).toBeLessThan(6 - 1);
  expect(sent("query") + sent("count_ui") + sent("dispatch")).toBe(BACKGROUND_SLOTS);

  await expect(t.pickFolder()).resolves.toBeNull();
  const forgotten = t.dispatch({
    command_id: "f",
    command: { forget_matter: { matter: "m" } },
  } as never);
  await settle();
  expect(sent("pick_folder_or_choose")).toBe(1);
  expect(
    invoked
      .filter((i) => i.command === "dispatch")
      .map((i) => (i.args as { command: { command_id: string } }).command.command_id),
  ).toEqual(["f"]);
  await forgotten;

  // once the desktop answers, the held reads go in the order they came
  for (let round = 0; round < 3; round += 1) {
    slow.release();
    await settle();
  }
  await Promise.all(reads);
  expect(invoked.slice(-3).map((i) => i.command)).toEqual(["query", "count_ui", "dispatch"]);
});

test("acks hold at most one request: what comes meanwhile goes in one ack once it answers", async () => {
  const slow = hold();
  answers.set("ack", slow.answer);
  const frames: (() => void)[] = [];
  const t = new TauriTransport({ warn: () => {}, nextFrame: (run) => frames.push(run) });
  t.subscribe("s1", null, { batch: () => {}, failed: () => {} });
  t.subscribe("s2", null, { batch: () => {}, failed: () => {} });
  await settle();
  t.ack("s1", { epoch: "e", cursor: 1 } as never);
  await settle();
  expect(sent("ack")).toBe(1);
  t.ack("s1", { epoch: "e", cursor: 2 } as never);
  t.ack("s2", { epoch: "e", cursor: 5 } as never);
  await settle();
  t.ack("s1", { epoch: "e", cursor: 3 } as never);
  await settle();
  expect(sent("ack")).toBe(1);
  slow.release();
  await settle();
  const acks = invoked.filter((i) => i.command === "ack").map((i) => i.args);
  expect(acks).toEqual([
    { positions: [{ stream: "s1", epoch: "e", cursor: 1 }], messages: 0 },
    {
      positions: [
        { stream: "s1", epoch: "e", cursor: 3 },
        { stream: "s2", epoch: "e", cursor: 5 },
      ],
      messages: 0,
    },
  ]);
  slow.release();
});
