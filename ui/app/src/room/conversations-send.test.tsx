// @vitest-environment happy-dom
// A new conversation's first send refused, in the real room (room.tsx over FakeTransport): a refused create_thread keeps
// the draft and its words; a refused send_prompt keeps the words, and the next Enter asks the same thread again.
import { afterEach, expect, test } from "vitest";
import { uuid } from "../testing/stream-script.ts";
import type { HostCommand, HostError, ThreadId, ThreadSummary } from "../transport/protocol.ts";
import { defaultOutcome, mountRoom, THE_MATTER, type Harness } from "./thread/harness.ts";

let h: Harness | null = null;
afterEach(async () => {
  await h?.unmount();
  h = null;
});

const T1 = uuid(1, 1);
const MADE = uuid(9, 1);
const QUESTION = "Summarise the indemnity clause";

const thread = (id: ThreadId, over: Partial<ThreadSummary> = {}): ThreadSummary => ({
  thread: id,
  matter: THE_MATTER.matter,
  agent: "claude-acp",
  status: "idle",
  ...over,
});

const refusal: HostError = { code: "internal", message: "for logs only", retryable: false };
const field = () => h!.one("composer") as HTMLTextAreaElement | null;
const kinds = () =>
  h!.transport.only("dispatch").map((c) => Object.keys(c.command.command)[0] as keyof HostCommand);
const newRow = () =>
  h!
    .all("conversation")
    .find((r) => r.querySelector(".room-conv-title")?.textContent === "New conversation");

test("create_thread refused: the draft row and the words stay, and nothing else is sent", async () => {
  h = await mountRoom({
    threads: [thread(T1, { title: "Lease notice" })],
    dispatch: (command) => {
      if (command.create_thread !== undefined) throw refusal;
      return defaultOutcome(command);
    },
  });
  await h.click(h.one("new-conversation"));
  await h.type(QUESTION);
  await h.key({ key: "Enter", keyCode: 13 });
  await h.settle();
  expect(kinds()).toEqual(["create_thread"]);
  expect(newRow()).toBeDefined();
  expect(newRow()?.getAttribute("aria-current")).toBe("true");
  expect(field()?.value).toBe(QUESTION);
});

test("send_prompt refused: the words stay, and the next Enter asks the same thread, with no second create_thread", async () => {
  let prompts = 0;
  h = await mountRoom({
    threads: [thread(T1, { title: "Lease notice" })],
    dispatch: (command) => {
      if (command.create_thread !== undefined) return { thread_created: { thread: MADE } };
      if (command.send_prompt !== undefined) {
        prompts += 1;
        if (prompts === 1) throw refusal;
      }
      return defaultOutcome(command);
    },
  });
  await h.click(h.one("new-conversation"));
  await h.type(QUESTION);
  await h.key({ key: "Enter", keyCode: 13 });
  await h.settle();
  expect(kinds()).toEqual(["create_thread", "send_prompt"]);
  expect(field()?.value).toBe(QUESTION);
  await h.key({ key: "Enter", keyCode: 13 });
  await h.settle();
  expect(h.sent("create_thread")).toHaveLength(1);
  expect(h.sent("send_prompt").map((p) => p.thread)).toEqual([MADE, MADE]);
  expect(kinds().slice(0, 3)).toEqual(["create_thread", "send_prompt", "send_prompt"]);
});
