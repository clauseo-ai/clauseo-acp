// @vitest-environment happy-dom
// U7's composer while Clauseo works, in the real room over FakeTransport: Enter queues the field's words (send_prompt
// with no delivery, which the host queues, H9) and clears the field once accepted; a queued prompt never holds the
// composer busy past the running turn; the hint over the field offers Send now (delivery steer) and Queue, says what
// Send now does by the header's steer_joins_turn and the agent, and hides while the conversation waits behind another;
// at the cap Queue is disabled and Enter keeps the words; a send refused as thread.queue_full says the cap; and with a
// permission waiting, Enter answers it only with the field empty (QU.9 a).
import { act } from "react";
import { afterEach, expect, test } from "vitest";
import {
  header,
  permission,
  reset,
  step,
  THREAD,
  toolCall,
  TURN,
  upsert,
  userMessage,
  uuid,
} from "../../testing/stream-script.ts";
import type {
  CommandOutcome,
  HostCommand,
  ThreadStatus,
  ThreadSummary,
  ViewEntry,
} from "../../transport/protocol.ts";
import { defaultOutcome, mountRoom, summary, type Harness } from "./harness.ts";

let h: Harness | null = null;
afterEach(async () => {
  await h?.unmount();
  h = null;
});

const QUESTION = "Also check the summoning order.";
const QUEUED_TURN = uuid(2, 4);
const CLAUDE_HINT = "Send now gives your words to Clauseo while it writes.";
const CHATGPT_HINT =
  "ChatGPT can't take words mid-answer. Send now stops this answer, keeps what it wrote, and sends your words first.";

const field = () => h!.one("composer") as HTMLTextAreaElement;
const hint = () => h!.one("steer-hint");
const hintText = () => hint()?.querySelector(".steer-hint-text")?.textContent ?? null;
const queueButton = () => h!.one("queue") as HTMLButtonElement | null;
const disabled = (b: HTMLButtonElement | null) =>
  b?.disabled === true || b?.getAttribute("aria-disabled") === "true";
const enter = () => h!.key({ key: "Enter", keyCode: 13 });

/** The thread's header, running, with H9's steering flag as given (absent for undefined) and its agent. */
const running = (
  joins?: boolean,
  agent = "claude-acp",
  status: ThreadStatus = "running",
): ViewEntry => ({
  header: {
    ...header().header!,
    agent,
    status,
    ...(joins === undefined ? {} : { steer_joins_turn: joins }),
  } as NonNullable<ViewEntry["header"]>,
});

/** A queued message of the thread, its own turn. */
const queued = (order: number, n: number, text = `Queued ${n}`): ViewEntry => ({
  user_message: {
    ...userMessage(order, n, text).user_message!,
    status: "queued",
    turn: uuid(100 + n, 4),
  },
});

/** send_prompt accepted for the queued turn; the rest as by default. */
const acceptQueued = (c: HostCommand): CommandOutcome =>
  c.send_prompt !== undefined
    ? { prompt_accepted: { turn: QUEUED_TURN, message: uuid(9, 5) } }
    : defaultOutcome(c);

test("Enter while Clauseo works queues the words: send_prompt with no delivery, and the field clears once accepted", async () => {
  h = await mountRoom({ dispatch: acceptQueued });
  h.deliver([reset([userMessage(1, 1, "Q")], running(true))]);
  await h.type(QUESTION);
  expect(await enter()).toBe(true);
  expect(h.sent("send_prompt")).toEqual([
    { thread: THREAD, content: [{ text: { text: QUESTION } }] },
  ]);
  expect(field().value).toBe("");
  // nothing more over an empty field: the hint is gone
  expect(hint()).toBeNull();
});

test("a prompt queued behind the running turn leaves the composer idle once that turn ends", async () => {
  h = await mountRoom({ dispatch: acceptQueued });
  h.deliver([reset([], header("idle"))]);
  // asked while idle: this one is the sent turn, busy until its step
  await h.type("Q");
  await enter();
  expect(h.one("cancel")).not.toBeNull();
  h.deliver([upsert(userMessage(1, 1, "Q")), upsert(running(true))]);
  // queued behind it (the host accepts it for its own turn)
  await h.type(QUESTION);
  await enter();
  expect(h.sent("send_prompt")).toHaveLength(2);
  h.deliver([upsert(queued(5, 2, QUESTION))]);
  // the running turn stops and the queue holds: nothing runs, so Ask is back, though the queued turn has no step
  h.deliver([
    upsert(step(9, { turn: TURN, stop: "cancelled" })),
    upsert({ header: { ...running(true).header!, status: "idle", queue_held: true } }),
  ]);
  expect(h.one("cancel")).toBeNull();
  expect(h.one("send")).not.toBeNull();
});

test("a prompt sent into a held queue doesn't hold the composer busy: the header's status says when it runs", async () => {
  h = await mountRoom({ dispatch: acceptQueued });
  h.deliver([
    reset([userMessage(1, 1, "Q"), step(2, { stop: "cancelled" }), queued(5, 2)], {
      header: { ...header("idle").header!, queue_held: true },
    }),
  ]);
  await h.type(QUESTION);
  await enter();
  expect(h.sent("send_prompt")).toEqual([
    { thread: THREAD, content: [{ text: { text: QUESTION } }] },
  ]);
  expect(field().value).toBe("");
  // accepted for a turn with no step yet, and no header since: still Ask, not Stop
  expect(h.one("cancel")).toBeNull();
  expect(h.one("send")).not.toBeNull();
  // the queue resumes: busy by the status
  h.deliver([upsert(running(true))]);
  expect(h.one("cancel")).not.toBeNull();
});

test("the hint's Queue queues the field's words as Enter does", async () => {
  h = await mountRoom({ dispatch: acceptQueued });
  h.deliver([reset([userMessage(1, 1, "Q")], running(true))]);
  await h.type(QUESTION);
  expect(hint()).not.toBeNull();
  await h.click(queueButton());
  expect(h.sent("send_prompt")).toEqual([
    { thread: THREAD, content: [{ text: { text: QUESTION } }] },
  ]);
  expect(field().value).toBe("");
});

test("the hint's Send now sends the words with delivery steer", async () => {
  h = await mountRoom({ dispatch: acceptQueued });
  h.deliver([reset([userMessage(1, 1, "Q")], running(true))]);
  await h.type(QUESTION);
  await h.click(h.one("steer-send-now"));
  expect(h.sent("send_prompt")).toEqual([
    { thread: THREAD, content: [{ text: { text: QUESTION } }], delivery: "steer" },
  ]);
  expect(field().value).toBe("");
});

test("the hint shows only while busy with words in the field", async () => {
  h = await mountRoom();
  h.deliver([reset([], header("idle"))]);
  await h.type(QUESTION);
  expect(hint()).toBeNull();
  h.deliver([upsert(running(true))]);
  expect(hint()).not.toBeNull();
  await h.type("");
  expect(hint()).toBeNull();
});

test.each([
  ["a steer that joins the turn", true, "claude-acp", CLAUDE_HINT],
  ["Codex, which can't take words mid-answer", false, "codex-acp", CHATGPT_HINT],
  ["an agent that doesn't join, not Codex", false, "fake-agent", null],
  ["steering not known", undefined, "claude-acp", null],
] as const)("the hint's words: %s", async (_, joins, agent, said) => {
  const own: ThreadSummary = { ...summary(), agent };
  h = await mountRoom({ threads: [own] });
  h.deliver([reset([userMessage(1, 1, "Q")], running(joins, agent))]);
  await h.type(QUESTION);
  expect(hint()).not.toBeNull();
  expect(hintText()).toBe(said);
  // with no words to say, Send now and Queue still stand
  expect(h.one("steer-send-now")).not.toBeNull();
  expect(queueButton()).not.toBeNull();
});

test("no hint while the conversation waits behind another", async () => {
  const OTHER = uuid(0, 1);
  h = await mountRoom({
    threads: [
      { ...summary(THREAD, { queued: { behind: OTHER } }), title: "Lease notice" },
      { ...summary(OTHER, "running"), title: "Draft petition" },
    ],
  });
  h.deliver([reset([], running(true, "claude-acp", { queued: { behind: OTHER } }))]);
  await h.type(QUESTION);
  expect(h.one("working")?.textContent).toContain("Draft petition");
  expect(hint()).toBeNull();
});

test("at ten queued, Queue is disabled and Enter queues nothing and keeps the words", async () => {
  h = await mountRoom();
  const ten = Array.from({ length: 10 }, (_, i) => queued(10 + i, 10 + i));
  h.deliver([reset([userMessage(1, 1, "Q"), ...ten], running(true))]);
  await h.type(QUESTION);
  expect(disabled(queueButton())).toBe(true);
  expect(await enter()).toBe(true);
  expect(h.sent("send_prompt")).toEqual([]);
  expect(field().value).toBe(QUESTION);
  // one gone: Queue takes a press again
  h.deliver([upsert({ user_message: { ...ten[9]!.user_message!, status: "removed" } })]);
  expect(disabled(queueButton())).toBe(false);
});

test("a send refused as thread.queue_full says the cap on the working line, never the host's text", async () => {
  h = await mountRoom({
    dispatch: (c) => {
      if (c.send_prompt !== undefined)
        throw { code: "thread.queue_full", message: "raw host text", retryable: false };
      return defaultOutcome(c);
    },
  });
  h.deliver([reset([userMessage(1, 1, "Q")], running(true))]);
  await h.type(QUESTION);
  await enter();
  await act(async () => h!.store.flush());
  expect(h.one("error")?.textContent).toBe("The queue holds 10.");
  expect(h.host.textContent).not.toContain("raw host text");
  expect(field().value).toBe(QUESTION);
});

test("with a permission waiting, Enter queues the field's words and answers nothing; with the field empty it answers", async () => {
  h = await mountRoom({ dispatch: acceptQueued });
  h.deliver([
    reset(
      [userMessage(1, 1, "Q"), toolCall(2, 1, "pending"), permission(3, 1)],
      running(true, "claude-acp", "needs_you"),
    ),
  ]);
  await h.type(QUESTION);
  expect(await enter()).toBe(true);
  expect(h.sent("send_prompt")).toEqual([
    { thread: THREAD, content: [{ text: { text: QUESTION } }] },
  ]);
  expect(h.transport.only("answer_permission")).toEqual([]);
  expect(field().value).toBe("");
  // the field empty: Enter answers the prompt (Allow once) and sends nothing more
  expect(await enter()).toBe(true);
  expect(h.transport.only("answer_permission").map((c) => c.option)).toEqual(["opt-allow_once"]);
  expect(h.sent("send_prompt")).toHaveLength(1);
});

/** The room's commits for each keystroke after the first, with the thread's header as given. */
async function keystrokeCommits(head: ViewEntry): Promise<number[]> {
  let commits = 0;
  h = await mountRoom({ onRender: () => void (commits += 1) });
  h.deliver([reset([userMessage(1, 1, "Q")], head)]);
  await h.type("A");
  const per: number[] = [];
  for (const text of ["Al", "Also", "Also check", "Also check the"]) {
    commits = 0;
    await h.type(text);
    per.push(commits);
  }
  await h.unmount();
  h = null;
  return per;
}

test("typing during an answer, with the hint up, commits no more per keystroke than typing while idle", async () => {
  const idle = await keystrokeCommits(header("idle"));
  const busy = await keystrokeCommits(running(true));
  expect(busy).toEqual(idle);
  // past the first keystroke's settling, none
  expect(busy.slice(1)).toEqual([0, 0, 0]);
});
