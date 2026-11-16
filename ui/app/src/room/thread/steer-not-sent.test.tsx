// @vitest-environment happy-dom
// U7's steer made honest, in the real room over FakeTransport: a steer the host never sent (`not_sent`) shows its
// words and that they were not sent, never the mark nor the waiting row, whatever its status; its words come back into
// the composer as it turns not sent (after the words there, on a new line), once, and never for one that came already
// not sent; and the note that the answer uses what a steer's background work found shows only for a steer the host
// confirmed taken in (`injected`).
import { mt } from "@clauseo/components/lib/words-matters.ts";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import {
  header,
  reset,
  step,
  toolCall,
  TURN,
  upsert,
  userMessage,
  uuid,
} from "../../testing/stream-script.ts";
import type {
  BackgroundMark,
  StepEntry,
  TurnId,
  UserMessageEntry,
  ViewEntry,
} from "../../transport/protocol.ts";
import { mountRoom, type Harness } from "./harness.ts";
import { forgetSteerRestores } from "./steer-restore.ts";

let h: Harness | null = null;
const NOW = Date.UTC(2026, 10, 3, 17, 0);
beforeEach(() => {
  vi.useFakeTimers({ toFake: ["Date"] });
  vi.setSystemTime(NOW);
});
afterEach(async () => {
  await h?.unmount();
  h = null;
  forgetSteerRestores();
  document.body.replaceChildren();
  vi.useRealTimers();
});

const T1 = TURN;
const T2: TurnId = uuid(2, 4);
const AT = NOW - 29 * 60_000;
const LATER = AT + 60_000;
const WORDS = "Say who opened the account.";
const NOT_SENT_LINE = "Not sent, because you stopped the answer.";

type Steer = NonNullable<UserMessageEntry["steer"]>;
const notSent = (turn: TurnId): Steer => ({ not_sent: { turn } });

const ask = (
  order: number,
  n: number,
  text: string,
  turn: TurnId,
  over: Partial<UserMessageEntry> = {},
): ViewEntry => ({
  user_message: { ...userMessage(order, n, text).user_message!, turn, ...over },
});
const steerAsk = (over: Partial<UserMessageEntry> = {}) =>
  ask(2, 2, WORDS, T1, { delivery: "steer", ts: AT, ...over });
const ended = (order: number, turn: TurnId, over: Partial<StepEntry> = {}): ViewEntry =>
  step(order, { turn, ...over });
const call = (order: number, n: number, turn: TurnId, background: BackgroundMark): ViewEntry => ({
  tool_call: { ...toolCall(order, n, "completed").tool_call!, turn, background },
});
const field = () => h!.one("composer") as HTMLTextAreaElement;
const steerId = `user_message:${uuid(2, 5)}`;

test.each([
  ["sent", {}],
  ["removed (the host ended its held turn)", { status: "removed" as const }],
])(
  "a steer never sent shows its words and that they were not sent, never the mark nor Send now: %s",
  async (_, over) => {
    h = await mountRoom();
    h.deliver([
      reset(
        [
          ask(1, 1, "Who signed the cheque?", T1),
          steerAsk({ steer: notSent(T1), ...over }),
          ended(3, T1, { stop: "cancelled" }),
        ],
        header("idle"),
      ),
    ]);
    await h.settle();
    const row = h.one("steer-not-sent");
    expect(row?.dataset["entryId"]).toBe(steerId);
    expect(row?.querySelector(".steer-mark-text")?.textContent).toBe(WORDS);
    expect(row?.querySelector(".steer-wait-when")?.textContent).toBe(mt("en", NOT_SENT_LINE));
    expect(row?.querySelector("button")).toBeNull();
    expect(h.one("steer-mark")).toBeNull();
    expect(h.one("steer-waiting")).toBeNull();
    expect(h.host.textContent).not.toContain("Sends when");
    // in the answer it was sent into, no question of its own
    expect(row?.closest("section")?.className).toBe("step step-answer");
    expect(h.all("message-user").map((m) => m.dataset["entryId"])).toEqual([
      `user_message:${uuid(1, 5)}`,
    ]);
  },
);

test("a steer that turns not sent comes back into the composer after its words, on a new line, once", async () => {
  h = await mountRoom();
  const waiting = steerAsk();
  h.deliver([reset([ask(1, 1, "Who signed the cheque?", T1), waiting], header("running"))]);
  await h.settle();
  expect(h.one("steer-waiting")).not.toBeNull();
  await h.type("Also the bank's name.");
  h.deliver([
    upsert({ user_message: { ...waiting.user_message!, steer: notSent(T1) } }),
    upsert(ended(3, T1, { stop: "cancelled" })),
  ]);
  await h.settle();
  expect(field().value).toBe(`Also the bank's name.\n${WORDS}`);
  expect(h.one("steer-not-sent")).not.toBeNull();
  // the same entry again (a new object, as a reset or a later flush brings it): nothing more
  h.deliver([upsert({ user_message: { ...waiting.user_message!, steer: notSent(T1) } })]);
  h.deliver([
    reset(
      [
        ask(1, 1, "Who signed the cheque?", T1),
        { user_message: { ...waiting.user_message!, steer: notSent(T1) } },
        ended(3, T1, { stop: "cancelled" }),
      ],
      header("idle"),
    ),
  ]);
  await h.settle();
  expect(field().value).toBe(`Also the bank's name.\n${WORDS}`);
});

test("into an empty composer the words come back as they were; a steer already not sent when it comes, never", async () => {
  h = await mountRoom();
  const waiting = steerAsk();
  // already not sent as the conversation opens: it stays in the transcript, the field empty
  h.deliver([
    reset(
      [
        ask(1, 1, "Who signed the cheque?", T1),
        ask(3, 3, "An older steer.", T1, { delivery: "steer", steer: notSent(T1) }),
        waiting,
      ],
      header("running"),
    ),
  ]);
  await h.settle();
  expect(field().value).toBe("");
  h.deliver([upsert({ user_message: { ...waiting.user_message!, steer: notSent(T1) } })]);
  await h.settle();
  expect(field().value).toBe(WORDS);
});

/** A steer's background call, finished before its answer ended, with the steer's outcome as given (none: waiting). */
async function withBackground(steer: Steer | undefined) {
  h = await mountRoom();
  h.deliver([
    reset(
      [
        ask(1, 1, "Read the petition.", T1),
        call(3, 1, T1, { steer: uuid(2, 5), finished_at: LATER }),
        ask(4, 2, "Check section 149(12) as well.", T1, {
          delivery: "steer",
          ts: AT,
          ...(steer === undefined ? {} : { steer }),
        }),
        ended(6, T1, { ts: LATER + 60_000 }),
      ],
      header("idle"),
    ),
  ]);
  await h.settle();
}

test("a steer taken in shows its mark and the note that the answer uses what it found", async () => {
  await withBackground({ injected: { turn: T1 } });
  expect(h!.one("steer-mark")).not.toBeNull();
  expect(h!.one("steer-note")?.textContent).toContain("The answer uses what it found.");
});

test.each([
  ["not sent", notSent(T1)],
  ["requeued", { requeued: { turn: T2 } } as Steer],
  ["sent now", { sent_now: { turn: T2 } } as Steer],
  ["still waiting", undefined],
])(
  "a steer not taken in never gets the note, background marks and an ended step or not: %s",
  async (_, steer) => {
    await withBackground(steer);
    expect(h!.one("steer-note")).toBeNull();
    expect(h!.host.textContent).not.toContain("The answer uses what it found");
  },
);
