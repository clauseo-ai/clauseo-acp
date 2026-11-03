// @vitest-environment happy-dom
// U7's transcript under a queue and a steer, in the real room over FakeTransport: queued and removed messages show in
// no exchange and are never the pinned question; a queued message whose turn starts later heads its own exchange,
// after the earlier turn's tail (its old order); a steer the agent took in stands in the running answer as a mark with
// its time; a requeued steer and a steer sent now say where they went, and the stopped answer stays; a tool call moved
// to the background says so, and the answer's note explains it once it finished; and a queued message coming, being
// edited or going renders no row of the transcript.
import { act, createElement, memo, Profiler, type ProfilerOnRenderCallback } from "react";
import { mt } from "@clauseo/components/lib/words-matters.ts";
import { th } from "@clauseo/components/lib/words-thread.ts";
import { afterEach, expect, test, vi } from "vitest";
import {
  agentText,
  append,
  header,
  reset,
  step,
  TURN,
  toolCall,
  upsert,
  userMessage,
  uuid,
} from "../../testing/stream-script.ts";
import type { BackgroundMark, UserMessageEntry } from "../../transport/protocol.ts";
import type { EntryId, StepEntry, StreamId, TurnId, ViewEntry } from "../../transport/protocol.ts";
import { clockTime } from "./clock.ts";
import { mountRoom, type Harness } from "./harness.ts";

// each transcript row inside its own Profiler, behind a memo like the real row's (room/queue.test.tsx's way)
const rowCommits = new Map<string, number>();
const countRow: ProfilerOnRenderCallback = (id) =>
  rowCommits.set(id, (rowCommits.get(id) ?? 0) + 1);
vi.mock("./rows.tsx", async (load) => {
  const real = await load<typeof import("./rows.tsx")>();
  return {
    ...real,
    Row: memo(function ProfiledRow(props: { id: EntryId; stream: StreamId; sentNow?: boolean }) {
      return createElement(
        Profiler,
        { id: props.id, onRender: countRow },
        createElement(real.Row, props),
      );
    }),
  };
});

let h: Harness | null = null;
afterEach(async () => {
  await h?.unmount();
  h = null;
  rowCommits.clear();
  document.body.replaceChildren();
});

const T1 = TURN;
const T2: TurnId = uuid(2, 4);
const T3: TurnId = uuid(3, 4);
/** Today, 4:31 pm on the local clock, and a minute on. */
const AT = new Date(new Date().setHours(16, 31, 0, 0)).getTime();
const LATER = AT + 60_000;

/** A user message of turn `turn`, with any of H9's fields. */
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
const ended = (order: number, turn: TurnId, over: Partial<StepEntry> = {}): ViewEntry =>
  step(order, { turn, ...over });
const call = (order: number, n: number, turn: TurnId, background?: BackgroundMark): ViewEntry => ({
  tool_call: {
    ...toolCall(order, n, "completed").tool_call!,
    turn,
    ...(background === undefined ? {} : { background }),
  },
});

const thread = () => h!.host.querySelector<HTMLElement>(".room-thread")!;
const shownIds = () =>
  [...thread().querySelectorAll<HTMLElement>("[data-entry-id]")].map((e) => e.dataset["entryId"]);
const pinned = () => h!.host.querySelector(".room-pin-text")?.textContent;
const asked = () => h!.all("message-user").map((m) => m.dataset["entryId"]);
const id = (kind: string, n: number, k: number) => `${kind}:${uuid(n, k)}`;

test("queued and removed messages show in no exchange and are never the pinned question", async () => {
  h = await mountRoom();
  h.deliver([
    reset(
      [
        ask(1, 1, "Read clause nine.", T1),
        say(2, 1, "Clause nine says", T1),
        ask(3, 2, "Then the lease.", T2, { status: "queued" }),
        ask(4, 3, "And the deed.", T3, { status: "removed" }),
      ],
      header("running"),
    ),
  ]);
  await h.settle();
  expect(asked()).toEqual([id("user_message", 1, 5)]);
  expect(thread().textContent).not.toContain("Then the lease.");
  expect(thread().textContent).not.toContain("And the deed.");
  expect(pinned()).toBe("Read clause nine.");
  // the answer runs on, in the first exchange
  expect(h.host.querySelectorAll("section.step-answer").length).toBe(1);
});

test("a queued message whose turn starts later heads its own exchange, after the earlier turn's tail", async () => {
  h = await mountRoom();
  const second = ask(3, 2, "Then the lease.", T2, { status: "queued" });
  h.deliver([
    reset(
      [
        ask(1, 1, "Read clause nine.", T1),
        say(2, 1, "Clause nine says notice.", T1),
        second,
        say(4, 3, "And the bank replied late.", T1),
        ended(5, T1),
      ],
      header("running"),
    ),
  ]);
  await h.settle();
  // its turn starts: only the status flips, the order stays 3
  h.deliver([
    upsert({ user_message: { ...second.user_message!, status: "sent" } }),
    upsert(say(6, 4, "The lease runs to 2027.", T2)),
  ]);
  await h.settle();
  expect(shownIds()).toEqual([
    id("user_message", 1, 5),
    id("agent_text", 1, 6),
    id("agent_text", 3, 6),
    `step:${T1}`,
    id("user_message", 2, 5),
    id("agent_text", 4, 6),
  ]);
  const sections = [...thread().querySelectorAll("section")].map((s) => s.className);
  expect(sections).toEqual([
    "step step-question",
    "step step-answer",
    "step step-question",
    "step step-answer",
  ]);
  expect(pinned()).toBe("Then the lease.");
});

test("a steer the agent took in stands in the running answer, where it arrived, with its time", async () => {
  h = await mountRoom();
  h.deliver([
    reset(
      [
        ask(1, 1, "Read clause nine.", T1),
        say(2, 1, "Clause nine says", T1),
        ask(3, 2, "Deal with the board minutes first.", T1, {
          delivery: "steer",
          steer: { injected: { turn: T1 } },
          ts: AT,
        }),
        say(4, 3, "The board minutes show", T1),
      ],
      header("running"),
    ),
  ]);
  await h.settle();
  const mark = h.one("steer-mark");
  expect(mark?.dataset["entryId"]).toBe(id("user_message", 2, 5));
  expect(mark?.querySelector(".steer-mark-who")?.textContent).toBe(
    mt("en", "You, {time}, while Clauseo answered", { time: clockTime("en", AT) }),
  );
  expect(mark?.querySelector(".steer-mark-text")?.textContent).toBe(
    "Deal with the board minutes first.",
  );
  // inside the one answer, between what came before and after it; never a question of its own
  const answers = thread().querySelectorAll("section.step-answer");
  expect(answers.length).toBe(1);
  expect(answers[0]!.contains(mark)).toBe(true);
  expect(shownIds()).toEqual([
    id("user_message", 1, 5),
    id("agent_text", 1, 6),
    id("user_message", 2, 5),
    id("agent_text", 3, 6),
  ]);
  expect(asked()).toEqual([id("user_message", 1, 5)]);
  expect(pinned()).toBe("Read clause nine.");
});

test("a steer not yet resolved is a mark in the running answer; requeued, it heads the next exchange and says why", async () => {
  h = await mountRoom();
  const steer = ask(3, 2, "Say who opened the account.", T2, { delivery: "steer", ts: AT });
  h.deliver([
    reset(
      [ask(1, 1, "Who signed the cheque?", T1), say(2, 1, "Mr. Mehta signed it.", T1), steer],
      header("running"),
    ),
  ]);
  await h.settle();
  expect(h.one("steer-mark")?.dataset["entryId"]).toBe(id("user_message", 2, 5));
  expect(h.one("steer-outcome")).toBeNull();
  h.deliver([
    upsert(ended(4, T1)),
    upsert({ user_message: { ...steer.user_message!, steer: { requeued: { turn: T2 } } } }),
    upsert(say(5, 3, "The account was opened by", T2)),
  ]);
  await h.settle();
  expect(h.one("steer-mark")).toBeNull();
  expect(asked()).toEqual([id("user_message", 1, 5), id("user_message", 2, 5)]);
  const line = h.one("steer-outcome");
  expect(line?.dataset["outcome"]).toBe("requeued");
  expect(line?.textContent).toBe(mt("en", "Your words came as the answer ended, so they go next."));
  // under the question it follows, in the new exchange
  expect(line?.closest("section")?.className).toBe("step step-question");
  expect(shownIds().slice(-2)).toEqual([id("user_message", 2, 5), id("agent_text", 3, 6)]);
});

test("a steer sent now: the stopped answer stays, says when it stopped and why, and the words head the next exchange", async () => {
  h = await mountRoom();
  h.deliver([
    reset(
      [
        ask(1, 1, "Who signed the cheque?", T1),
        say(2, 1, "Accused no. 2, Mr. Rakesh Mehta.", T1),
        ask(3, 2, "Say too who opened the account.", T2, {
          delivery: "steer",
          steer: { sent_now: { turn: T2 } },
        }),
        ended(4, T1, { stop: "cancelled", ts: LATER }),
        say(5, 3, "Reading the matter's papers.", T2),
      ],
      header("running"),
    ),
  ]);
  await h.settle();
  const line = h.one("steer-outcome");
  expect(line?.dataset["outcome"]).toBe("sent_now");
  // the design's stop line type, as the note's text (matters.css)
  expect(line?.className).toBe("text-small-row text-muted");
  expect(line?.textContent).toBe(
    mt("en", "Stopped at {time} to send your words first. The answer so far is kept.", {
      time: clockTime("en", LATER),
    }),
  );
  // in the stopped turn's step, in place of its own stop line
  expect(line?.closest("[data-testid=turn-stopped]")?.getAttribute("data-entry-id")).toBe(
    `step:${T1}`,
  );
  expect(thread().textContent).not.toContain(
    th("en", "You stopped the answer. The answer so far is kept."),
  );
  expect(thread().textContent).toContain("Accused no. 2, Mr. Rakesh Mehta.");
  expect(asked()).toEqual([id("user_message", 1, 5), id("user_message", 2, 5)]);
  expect(shownIds()).toEqual([
    id("user_message", 1, 5),
    id("agent_text", 1, 6),
    `step:${T1}`,
    id("user_message", 2, 5),
    id("agent_text", 3, 6),
  ]);
  expect(pinned()).toBe("Say too who opened the account.");
});

test("a tool call moved to the background says so; the answer's note explains it once it finished and the answer ended", async () => {
  h = await mountRoom();
  const steerId = uuid(2, 5);
  const moved = call(3, 1, T1, { steer: steerId });
  h.deliver([
    reset(
      [
        ask(1, 1, "Read the petition.", T1),
        say(2, 1, "The petition says", T1),
        moved,
        ask(4, 2, "Check section 149(12) as well.", T1, {
          delivery: "steer",
          steer: { injected: { turn: T1 } },
          ts: AT,
        }),
        say(5, 3, "Section 149(12) asks", T1),
      ],
      header("running"),
    ),
  ]);
  await h.settle();
  // its row, in its step group opened, says so
  await h.click(h.one("step-group")?.querySelector("button"));
  const row = h.all("tool-call")[0]!;
  expect(row.dataset["background"]).toBe("true");
  expect(row.textContent).toContain(mt("en", "In the background"));
  // still running: no note yet
  expect(h.one("steer-note")).toBeNull();
  h.deliver([upsert(call(3, 1, T1, { steer: steerId, finished_at: LATER }))]);
  await h.settle();
  // finished, but the answer runs on: no step yet, so no note
  expect(h.one("steer-note")).toBeNull();
  h.deliver([upsert(ended(6, T1, { ts: LATER + 60_000 }))]);
  await h.settle();
  const note = h.one("steer-note");
  const text = note?.querySelector(".steer-note-text")?.textContent;
  expect(text).toBe(
    mt(
      "en",
      "Your words at {time} came in before some work had finished. It went on in the background and finished at {done}. The answer uses what it found.",
      { time: clockTime("en", AT), done: clockTime("en", LATER) },
    ),
  );
  // the call's own title is the list's, not the sentence's
  expect(text).not.toContain("Read file 1");
  expect(note?.closest("section")?.className).toBe("step step-answer");
  const list = note!.querySelector("ul")!;
  expect(list.hidden).toBe(true);
  const shown: Element[] = [];
  const into = vi.spyOn(HTMLElement.prototype, "scrollIntoView").mockImplementation(function (
    this: HTMLElement,
  ) {
    shown.push(this);
  });
  await h.click(note!.querySelector("button"));
  expect(list.hidden).toBe(false);
  // opened at the thread's foot, the list comes into view
  expect(shown).toContain(list);
  into.mockRestore();
  expect([...list.querySelectorAll("li")].map((li) => li.textContent)).toEqual([
    `${clockTime("en", LATER)}Read file 1`,
  ]);
});

test("a queued message coming, being edited and going renders no row of the transcript", async () => {
  h = await mountRoom();
  h.deliver([
    reset(
      [ask(1, 1, "Read clause nine.", T1), say(2, 1, "Clause nine says", T1)],
      header("running"),
    ),
  ]);
  await h.settle();
  expect(rowCommits.size).toBeGreaterThan(0);
  rowCommits.clear();
  const queued = ask(3, 2, "Then the lease.", T2, { status: "queued" });
  h.deliver([upsert(queued)]);
  await h.settle();
  h.deliver([
    upsert({
      user_message: {
        ...queued.user_message!,
        content: [{ text: { text: "Then the lease, and the deed." } }],
        edited: true,
      },
    }),
  ]);
  await h.settle();
  h.deliver([upsert({ user_message: { ...queued.user_message!, status: "removed" } })]);
  await h.settle();
  expect([...rowCommits.entries()]).toEqual([]);
  // the answer streaming on renders its own row alone
  h.deliver([append(1, "Clause nine says".length, " notice.")]);
  await h.settle();
  expect([...rowCommits.keys()]).toEqual([id("agent_text", 1, 6)]);
  await act(async () => {});
});

/** The room over a window that has older entries, page_entries answering from `page`. */
async function paged(
  page: ViewEntry[],
  window: ViewEntry[],
  status: "idle" | "running" = "idle",
): Promise<Harness> {
  const room = await mountRoom({
    query: (query) =>
      typeof query === "object" && query.page_entries !== undefined
        ? { entries: page }
        : { entries: [] },
  });
  const body = reset(window, header(status));
  body.reset!.view_window.has_older = true;
  room.deliver([body]);
  await room.settle();
  return room;
}

const sectionsOf = () => [...thread().querySelectorAll("section")].map((s) => s.className);

test("a window that starts on a queued question, the earlier turn's tail after it: once the page is in, each turn under its question", async () => {
  h = await paged(
    [ask(1, 1, "Read clause nine.", T1), say(2, 1, "Clause nine says", T1)],
    [
      ask(3, 2, "Then the lease.", T2),
      say(4, 3, " notice.", T1),
      ended(5, T1),
      say(6, 4, "The lease runs to 2027.", T2),
    ],
  );
  expect(shownIds()).toEqual([
    id("user_message", 1, 5),
    id("agent_text", 1, 6),
    id("agent_text", 3, 6),
    `step:${T1}`,
    id("user_message", 2, 5),
    id("agent_text", 4, 6),
  ]);
  expect(sectionsOf()).toEqual([
    "step step-question",
    "step step-answer",
    "step step-question",
    "step step-answer",
  ]);
  expect(pinned()).toBe("Then the lease.");
});

test("a window that starts on a turn's tail, the next question on the page: once the page is in, the next answer goes under it", async () => {
  h = await paged(
    [
      ask(1, 1, "Read clause nine.", T1),
      say(2, 1, "Clause nine says", T1),
      ask(3, 2, "Then the lease.", T2),
    ],
    [say(4, 3, " notice.", T1), ended(5, T1), say(6, 4, "The lease runs to 2027.", T2)],
  );
  expect(shownIds()).toEqual([
    id("user_message", 1, 5),
    id("agent_text", 1, 6),
    id("agent_text", 3, 6),
    `step:${T1}`,
    id("user_message", 2, 5),
    id("agent_text", 4, 6),
  ]);
  expect(sectionsOf()).toEqual([
    "step step-question",
    "step step-answer",
    "step step-question",
    "step step-answer",
  ]);
  expect(thread().querySelectorAll("section")[3]?.textContent).toContain("The lease runs to 2027.");
});

test("a call that finished after its answer ended gives that answer no note", async () => {
  h = await mountRoom();
  const steerId = uuid(2, 5);
  h.deliver([
    reset(
      [
        ask(1, 1, "Read the petition.", T1),
        call(2, 1, T1, { steer: steerId, finished_at: LATER + 60_000 }),
        ask(3, 2, "Check section 149(12) as well.", T1, {
          delivery: "steer",
          steer: { injected: { turn: T1 } },
          ts: AT,
        }),
        say(4, 3, "Section 149(12) asks", T1),
        ended(5, T1, { ts: LATER }),
      ],
      header("idle"),
    ),
  ]);
  await h.settle();
  expect(h.one("steer-mark")).not.toBeNull();
  await h.click(h.one("step-group")?.querySelector("button"));
  expect(h.all("tool-call")[0]?.dataset["background"]).toBe("true");
  expect(h.one("steer-note")).toBeNull();
});

test("a steer still unresolved on an older page is placed as a live one: a mark while a turn runs, else a question", async () => {
  const page = [
    ask(1, 1, "Who signed the cheque?", T1),
    say(2, 1, "Mr. Mehta signed it.", T1),
    ask(3, 2, "Say who opened the account.", T2, { delivery: "steer", ts: AT }),
  ];
  const window = [say(4, 3, " He signed on 3 May.", T1)];
  const steerId = id("user_message", 2, 5);
  h = await paged(page, window, "running");
  // inside the one running answer, as its mark; never a question of its own
  expect(h.one("steer-mark")?.dataset["entryId"]).toBe(steerId);
  expect(h.one("steer-mark")?.closest("section")?.className).toBe("step step-answer");
  expect(asked()).toEqual([id("user_message", 1, 5)]);
  expect(sectionsOf()).toEqual(["step step-question", "step step-answer"]);
  await h.unmount();
  document.body.replaceChildren();
  h = await paged(page, window, "idle");
  // no turn runs: a question heading its own exchange, the grouping and the row agreeing
  expect(h.one("steer-mark")).toBeNull();
  expect(asked()).toEqual([id("user_message", 1, 5), steerId]);
  const question = h.all("message-user").find((m) => m.dataset["entryId"] === steerId);
  expect(question?.closest("section")?.className).toBe("step step-question");
});
