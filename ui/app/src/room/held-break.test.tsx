// @vitest-environment happy-dom
// U7 with U10 (matters/held-failed, matters/held): a message waiting in the queue is no new turn. After a turn fails
// with messages queued, before or after the failure, the stopped line stays and the field gets back the question that
// failed (U10), never the queued words; after Stop with a message queued, the strip says the queue is held and the
// field stays empty.
import { act } from "react";
import { afterEach, expect, test } from "vitest";
import {
  agentText,
  header,
  reset,
  step,
  upsert,
  userMessage,
  uuid,
} from "../testing/stream-script.ts";
import type { ViewEntry } from "../transport/protocol.ts";
import { loadQueueStrip } from "./queue-chunk.ts";
import { defaultOutcome, mountRoom, type Harness } from "./thread/harness.ts";

let h: Harness | null = null;
afterEach(async () => {
  await h?.unmount();
  h = null;
  document.body.replaceChildren();
});

const QUESTION = "Summarise the notice to the bank.";
const field = () => h!.one("composer") as HTMLTextAreaElement;
const thread = () => h!.host.querySelector<HTMLElement>(".room-thread")!;

const head = (status: "running" | "idle" | "agent_exited", held = false): ViewEntry => ({
  header: { ...header().header!, status, ...(held ? { queue_held: true } : {}) },
});
const queued = (order: number, n: number, text: string): ViewEntry => ({
  user_message: {
    ...userMessage(order, n, text).user_message!,
    status: "queued",
    turn: uuid(n + 10, 4),
  },
});

/** The room with the question asked and under way, each send its own turn. */
async function asked(): Promise<Harness> {
  let sends = 0;
  const room = await mountRoom({
    dispatch: (command) => {
      if (command.send_prompt === undefined) return defaultOutcome(command);
      sends += 1;
      return { prompt_accepted: { turn: uuid(sends, 4), message: uuid(sends, 5) } };
    },
  });
  await act(async () => void (await loadQueueStrip()));
  room.deliver([reset([])]);
  await room.type(QUESTION);
  await room.key({ key: "Enter", keyCode: 13 });
  room.deliver([
    upsert({ user_message: { ...userMessage(5, 1, QUESTION).user_message!, turn: uuid(1, 4) } }),
    upsert({
      agent_text: {
        ...agentText(6, 1, "Clause nine requires notice").agent_text!,
        turn: uuid(1, 4),
      },
    }),
    upsert(head("running")),
  ]);
  await room.settle();
  return room;
}

test("a turn failing with a message queued keeps its stopped line, and the field gets the failed question, not the queued words", async () => {
  h = await asked();
  h.deliver([upsert(queued(7, 2, "Then the lease."))]);
  await h.settle();
  h.deliver([
    upsert(step(9, { turn: uuid(1, 4), stop: "failed", failure: "agent.exited", ts: Date.now() })),
    upsert(head("agent_exited", true)),
  ]);
  await h.settle();
  expect(h.one("agent-stopped")?.dataset["code"]).toBe("agent.exited");
  expect(field().value).toBe(QUESTION);
  expect(thread().textContent).not.toContain("Then the lease.");
  // a message queued after the failure is no new turn either: the line stays, the field is as it was
  await act(async () => field().select());
  h.deliver([upsert(queued(10, 3, "And the deed."))]);
  await h.settle();
  expect(h.one("agent-stopped")).not.toBeNull();
  expect(field().value).toBe(QUESTION);
  expect(thread().textContent).not.toContain("And the deed.");
});

test("a turn failing with a code asking again can't mend: the step's words stay, the field stays empty though messages wait", async () => {
  h = await asked();
  h.deliver([
    upsert(queued(7, 2, "Then the lease.")),
    upsert(step(9, { turn: uuid(1, 4), stop: "failed", failure: "agent.protocol_error" })),
    upsert(head("idle", true)),
  ]);
  await h.settle();
  expect(field().value).toBe("");
  expect(h.one("queue-say")).not.toBeNull();
});

test("after Stop with a message queued, the strip says the queue is held and the field stays empty", async () => {
  h = await asked();
  h.deliver([upsert(queued(7, 2, "Then the lease."))]);
  await h.settle();
  await h.click(h.one("cancel"));
  expect(h.sent("cancel").length).toBe(1);
  h.deliver([upsert(step(9, { turn: uuid(1, 4), stop: "cancelled" })), upsert(head("idle", true))]);
  await h.settle();
  expect(h.one("queue-say")?.textContent).toContain("The queue is held. You stopped the answer at");
  expect(h.all("queued").map((r) => r.textContent)).toEqual([
    expect.stringContaining("Then the lease."),
  ]);
  expect(field().value).toBe("");
  expect(h.one("agent-stopped")).toBeNull();
  expect(thread().textContent).not.toContain("Then the lease.");
  expect(thread().textContent).toContain("Clause nine requires notice");
});

test("Enter while Clauseo works queues the words; they stand in the strip, then head their own exchange after the turn's tail", async () => {
  h = await asked();
  await h.type("Then the lease.");
  await h.key({ key: "Enter", keyCode: 13 });
  expect(h.sent("send_prompt").at(-1)?.content).toEqual([{ text: { text: "Then the lease." } }]);
  const waiting = queued(7, 2, "Then the lease.");
  h.deliver([upsert(waiting)]);
  await h.settle();
  expect(h.all("queued").length).toBe(1);
  expect(h.all("message-user").length).toBe(1);
  h.deliver([
    upsert({
      agent_text: {
        ...agentText(8, 3, "and the bank replied late.").agent_text!,
        turn: uuid(1, 4),
      },
    }),
    upsert(step(9, { turn: uuid(1, 4) })),
    upsert({ user_message: { ...waiting.user_message!, status: "sent" } }),
    upsert({
      agent_text: { ...agentText(10, 4, "The lease runs to 2027.").agent_text!, turn: uuid(12, 4) },
    }),
  ]);
  await h.settle();
  expect(h.all("queued").length).toBe(0);
  const ids = [...thread().querySelectorAll<HTMLElement>("[data-entry-id]")].map(
    (e) => e.dataset["entryId"],
  );
  expect(ids).toEqual([
    `user_message:${uuid(1, 5)}`,
    `agent_text:${uuid(1, 6)}`,
    `agent_text:${uuid(3, 6)}`,
    `step:${uuid(1, 4)}`,
    `user_message:${uuid(2, 5)}`,
    `agent_text:${uuid(4, 6)}`,
  ]);
});
