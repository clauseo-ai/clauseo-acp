// @vitest-environment happy-dom
// The step group under U7's queue and steer, in the real room over FakeTransport: a step a steer moved to the
// background says so on its row inside the opened group and on the folded line; a steer's mark parts the group either
// side of it; and a group never spans two exchanges, even where a queued message's question stands between its rows
// in the log.
import { mt } from "@clauseo/components/lib/words-matters.ts";
import { steps } from "@clauseo/components/lib/words-steps.ts";
import { afterEach, expect, test } from "vitest";
import {
  agentText,
  header,
  reset,
  step,
  TURN,
  toolCall,
  userMessage,
  uuid,
} from "../../testing/stream-script.ts";
import type {
  BackgroundMark,
  TurnId,
  UserMessageEntry,
  ViewEntry,
} from "../../transport/protocol.ts";
import { mountRoom, type Harness } from "./harness.ts";

let h: Harness | null = null;
afterEach(async () => {
  await h?.unmount();
  h = null;
  document.body.replaceChildren();
});

const T1 = TURN;
const T2: TurnId = uuid(2, 4);

const ask = (
  order: number,
  n: number,
  text: string,
  turn: TurnId,
  over: Partial<UserMessageEntry> = {},
): ViewEntry => ({
  user_message: { ...userMessage(order, n, text).user_message!, turn, ...over },
});
const say = (order: number, n: number, text: string, turn: TurnId): ViewEntry => ({
  agent_text: { ...agentText(order, n, text).agent_text!, turn },
});
const call = (order: number, n: number, turn: TurnId, background?: BackgroundMark): ViewEntry => ({
  tool_call: {
    ...toolCall(order, n, "completed").tool_call!,
    turn,
    ...(background === undefined ? {} : { background }),
  },
});
/** A steer the agent took into turn T1's answer. */
const steer = (order: number, n: number) =>
  ask(order, n, "Check section 149(12) as well.", T1, {
    delivery: "steer",
    steer: { injected: { turn: T1 } },
  });

const groups = () => h!.all("step-group");
/** The tool call ids of a group's rows, once it is opened. */
async function rowsOf(group: HTMLElement | undefined) {
  await h!.click(group?.querySelector("button"));
  return [...(group?.querySelectorAll<HTMLElement>('[data-testid="tool-call"]') ?? [])].map(
    (r) => r.dataset["toolCallId"],
  );
}

test("a step a steer moved to the background says so on its row inside the opened group, and the folded line keeps it in view", async () => {
  h = await mountRoom();
  h.deliver([
    reset(
      [
        ask(1, 1, "Read the petition.", T1),
        call(2, 1, T1),
        call(3, 2, T1, { steer: uuid(4, 5) }),
        steer(4, 4),
        say(5, 3, "Section 149(12) asks", T1),
        step(6, { turn: T1 }),
      ],
      header("idle"),
    ),
  ]);
  await h.settle();
  const [group] = groups();
  // folded: the line says one of its steps went on in the background, muted, not as trouble
  expect(group?.querySelector(".step-group-more")?.textContent).toBe(
    steps("en", "1 of them went on in the background."),
  );
  expect(group?.dataset["trouble"]).toBeUndefined();
  // opened: that row carries U7's mark, the other none
  await h.click(group?.querySelector("button"));
  const rows = h.all("tool-call");
  expect(rows.map((r) => r.dataset["background"])).toEqual([undefined, "true"]);
  expect(rows[1]?.textContent).toContain(mt("en", "In the background"));
  expect(rows[0]?.textContent).not.toContain(mt("en", "In the background"));
});

test("a steer's mark parts the group either side of it", async () => {
  h = await mountRoom();
  h.deliver([
    reset(
      [
        ask(1, 1, "Read the petition.", T1),
        call(2, 1, T1),
        call(3, 2, T1),
        steer(4, 4),
        call(5, 3, T1),
        say(6, 3, "Section 149(12) asks", T1),
      ],
      header("running"),
    ),
  ]);
  await h.settle();
  const [before, after] = groups();
  expect(groups()).toHaveLength(2);
  const mark = h.one("steer-mark")!;
  // in order: the first group, the mark, the second group
  expect(before!.compareDocumentPosition(mark) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  expect(mark.compareDocumentPosition(after!) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  expect(await rowsOf(before)).toEqual([uuid(1, 7), uuid(2, 7)]);
  expect(await rowsOf(after)).toEqual([uuid(3, 7)]);
});

test("a group never spans two exchanges: a turn's steps group on either side of a later question in the log", async () => {
  h = await mountRoom();
  h.deliver([
    reset(
      [
        ask(1, 1, "Read clause nine.", T1),
        call(2, 1, T1),
        // a queued message, sent once T1 ended, keeps the place it was sent at
        ask(3, 2, "Then the lease.", T2),
        call(4, 2, T1),
        step(5, { turn: T1 }),
        call(6, 3, T2),
        say(7, 3, "The lease says", T2),
      ],
      header("idle"),
    ),
  ]);
  await h.settle();
  const answers = [...h.host.querySelectorAll<HTMLElement>("section.step-answer")];
  expect(answers).toHaveLength(2);
  const [first, second] = groups();
  expect(groups()).toHaveLength(2);
  // T1's two steps fold together in T1's answer, T2's one in T2's
  expect(answers[0]!.contains(first!)).toBe(true);
  expect(answers[1]!.contains(second!)).toBe(true);
  expect(await rowsOf(first)).toEqual([uuid(1, 7), uuid(2, 7)]);
  expect(await rowsOf(second)).toEqual([uuid(3, 7)]);
});
