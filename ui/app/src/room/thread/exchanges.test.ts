// The thread's exchanges by turn (exchanges.ts): queued and removed messages in none, a question that starts later
// after the earlier turn's tail, a steer taken in inside the running answer, the turn a steer sent now stopped, a
// window that opens mid-turn, and the pick's identity while nothing that groups changes.
import { expect, test } from "vitest";
import { agentText, step, TURN, userMessage, uuid } from "../../testing/stream-script.ts";
import type { StreamView } from "../../store/view.ts";
import { entryIdOf } from "../../store/view.ts";
import type { UserMessageEntry } from "../../transport/protocol.ts";
import type { EntryId, TurnId, ViewEntry } from "../../transport/protocol.ts";
import { exchangesPick, groupExchanges, latestAsked } from "./exchanges.ts";

const T1 = TURN;
const T2: TurnId = uuid(2, 4);
const T3: TurnId = uuid(3, 4);

const ask = (
  order: number,
  n: number,
  turn: TurnId,
  over: Partial<UserMessageEntry> = {},
): ViewEntry => ({
  user_message: { ...userMessage(order, n, `q${n}`).user_message!, turn, ...over },
});
const say = (order: number, n: number, turn: TurnId): ViewEntry => ({
  agent_text: { ...agentText(order, n, `a${n}`).agent_text!, turn },
});
const notice = (order: number, n: number): ViewEntry => ({
  notice: { order, event: uuid(n, 9), level: "info", text: "n", ts: 0 },
});

const q = (n: number) => `user_message:${uuid(n, 5)}`;
const a = (n: number) => `agent_text:${uuid(n, 6)}`;
const s = (turn: TurnId) => `step:${turn}`;

const group = (entries: ViewEntry[], running = false) =>
  groupExchanges(
    entries.map((e) => [entryIdOf(e), e] as const),
    running,
  ).map((ex) => ({ asked: ex.asked, ids: ex.ids, sentNow: ex.sentNow }));

test("each sent question heads its turn's exchange; queued and removed messages are in none", () => {
  expect(
    group([
      ask(1, 1, T1),
      say(2, 1, T1),
      ask(3, 2, T2), // was queued: its order is inside the first turn
      notice(4, 1), // the first turn still runs: it joins that one
      say(5, 3, T1),
      step(6, { turn: T1 }),
      ask(7, 3, T3, { status: "queued" }),
      ask(8, 4, T3, { status: "removed" }),
      say(9, 4, T2),
    ]),
  ).toEqual([
    { asked: true, ids: [q(1), a(1), "notice:" + uuid(1, 9), a(3), s(T1)], sentNow: null },
    { asked: true, ids: [q(2), a(4)], sentNow: null },
  ]);
});

test("a question sent ahead of one queued earlier (send now) still follows the turn that ran before it", () => {
  // q2 waits; q3 goes first (its turn T3 runs from order 6), then q2's turn T2 from order 9
  expect(
    group([
      ask(1, 1, T1),
      say(2, 1, T1),
      ask(3, 2, T2),
      ask(4, 3, T3),
      step(5, { turn: T1 }),
      say(6, 3, T3),
      step(7, { turn: T3 }),
      say(9, 2, T2),
    ]).map((ex) => ex.ids[0]),
  ).toEqual([q(1), q(3), q(2)]);
});

test("a steer taken in joins the running answer by order; one sent now marks the turn it stopped", () => {
  expect(
    group([
      ask(1, 1, T1),
      say(2, 1, T1),
      ask(3, 2, T1, { delivery: "steer", steer: { injected: { turn: T1 } } }),
      say(4, 3, T1),
    ]),
  ).toEqual([{ asked: true, ids: [q(1), a(1), q(2), a(3)], sentNow: null }]);
  expect(
    group([
      ask(1, 1, T1),
      say(2, 1, T1),
      ask(3, 2, T2, { delivery: "steer", steer: { sent_now: { turn: T2 } } }),
      step(4, { turn: T1, stop: "cancelled" }),
      say(5, 3, T2),
    ]),
  ).toEqual([
    { asked: true, ids: [q(1), a(1), s(T1)], sentNow: s(T1) },
    { asked: true, ids: [q(2), a(3)], sentNow: null },
  ]);
});

test("a steer not yet resolved is a mark while a turn runs, a question when none does", () => {
  const entries = [ask(1, 1, T1), say(2, 1, T1), ask(3, 2, T2, { delivery: "steer" })];
  expect(group(entries, true)).toEqual([{ asked: true, ids: [q(1), a(1), q(2)], sentNow: null }]);
  expect(group(entries, false).map((ex) => ex.ids)).toEqual([[q(1), a(1)], [q(2)]]);
});

test("a window that opens mid-turn starts with an exchange without a question", () => {
  expect(group([say(2, 1, T1), step(3, { turn: T1 }), ask(4, 2, T2), say(5, 2, T2)])).toEqual([
    { asked: false, ids: [a(1), s(T1)], sentNow: null },
    { asked: true, ids: [q(2), a(2)], sentNow: null },
  ]);
});

/** A view as the store holds one, enough for the pick. */
const viewOf = (
  entries: ViewEntry[],
  running = true,
): StreamView & { entries: Map<EntryId, ViewEntry> } => {
  const ids: EntryId[] = entries.map(entryIdOf);
  return {
    ids,
    entries: new Map(entries.map((e) => [entryIdOf(e), e])),
    texts: new Map(),
    header: running ? { header: { status: "running" } } : null,
  } as unknown as StreamView & { entries: Map<EntryId, ViewEntry> };
};

test("the pick keeps its shape while only words or a queued message's text change, and builds again on a flip", () => {
  const pick = exchangesPick();
  const queued = ask(3, 2, T2, { status: "queued" });
  const v = viewOf([ask(1, 1, T1), say(2, 1, T1), queued]);
  const first = pick(v);
  expect(first.exchanges.map((ex) => ex.ids)).toEqual([[q(1), a(1)]]);
  expect(latestAsked(first.exchanges)).toBe(q(1));
  // an edit of the queued message: a new entry object, its words alone changed
  v.entries.set(q(2), {
    user_message: {
      ...queued.user_message!,
      content: [{ text: { text: "edited" } }],
      edited: true,
    },
  });
  expect(pick(v)).toBe(first);
  // its turn starts: the status flips, and it heads its own exchange
  v.entries.set(q(2), { user_message: { ...queued.user_message!, status: "sent" } });
  const next = pick(v);
  expect(next).not.toBe(first);
  expect(next.exchanges[0]).toBe(first.exchanges[0]);
  expect(latestAsked(next.exchanges)).toBe(q(2));
});

test("a window that starts with a queued question then the earlier turn's tail: the tail opens its own exchange first", () => {
  expect(group([ask(3, 2, T2), say(4, 1, T1), step(5, { turn: T1 }), say(6, 2, T2)])).toEqual([
    { asked: false, ids: [a(1), s(T1)], sentNow: null },
    { asked: true, ids: [q(2), a(2)], sentNow: null },
  ]);
});

test("a window that starts with one turn's tail, the next turn's question on the page: two exchanges without one", () => {
  expect(group([say(4, 1, T1), step(5, { turn: T1 }), say(6, 2, T2)])).toEqual([
    { asked: false, ids: [a(1), s(T1)], sentNow: null },
    { asked: false, ids: [a(2)], sentNow: null },
  ]);
});

test("a steer sent now that came as a turn had ended stops none: that step keeps its own line", () => {
  expect(
    group([
      ask(1, 1, T1),
      say(2, 1, T1),
      step(3, { turn: T1 }),
      ask(4, 2, T2, { delivery: "steer", steer: { sent_now: { turn: T2 } } }),
      say(5, 2, T2),
    ]).map((ex) => ex.sentNow),
  ).toEqual([null, null]);
});

test("the older pages and the window group as one: a turn begun on a page goes on in the window", () => {
  const page = [ask(1, 1, T1), say(2, 1, T1), ask(3, 2, T2)];
  const shape = exchangesPick(page)(viewOf([say(4, 3, T1), step(5, { turn: T1 }), say(6, 2, T2)]));
  expect(shape.exchanges.map((ex) => ({ asked: ex.asked, ids: ex.ids }))).toEqual([
    { asked: true, ids: [q(1), a(1), a(3), s(T1)] },
    { asked: true, ids: [q(2), a(2)] },
  ]);
  expect([...shape.paged.keys()]).toEqual([q(1), a(1), q(2)]);
  expect(latestAsked(shape.exchanges)).toBe(q(2));
  // a page entry the window holds too (from its first order on) is the window's
  const overlap = exchangesPick([...page, say(4, 3, T1)])(
    viewOf([say(4, 3, T1), step(5, { turn: T1 })]),
  );
  expect([...overlap.paged.keys()]).toEqual([q(1), a(1), q(2)]);
});
