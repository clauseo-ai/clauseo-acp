// The open matter's conversations on the rail in rail-order.ts's order: a permission or a question first, then an answer
// ready and unseen, then a failure unseen, then what's answering, then the rest by latest activity; held while the
// pointer is on the rail, settled when it leaves; and an answer ready leaving Needs you once opened, its mark_seen sent.
// @vitest-environment happy-dom
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { uuid } from "../testing/stream-script.ts";
import type { ConversationRow } from "./conversations.ts";
import { detailed } from "./fake-next.ts";
import type { MatterInfo, ThreadInfo } from "./host-next.ts";
import { railView } from "./rail-lists.ts";
import { ConversationOrder, conversationRank, orderConversations } from "./rail-order.ts";
import { LISTS } from "./rail-needs-rig.ts";
import {
  A,
  conversations,
  hover,
  mount,
  NOW,
  press,
  thread,
  titles,
  unmount,
} from "./rail-snooze-rig.ts";

beforeEach(() => {
  railView.set("matters");
  vi.useFakeTimers({ toFake: ["Date"] });
  vi.setSystemTime(NOW);
});

afterEach(async () => {
  await unmount();
  vi.useRealTimers();
  vi.restoreAllMocks();
});

const row = (key: string, over: Partial<ConversationRow> = {}): ConversationRow => ({
  key,
  thread: uuid(Number(key.replace(/\D/g, "")) || 1, 3),
  agent: "claude-code",
  status: "idle",
  seen: true,
  ...over,
});
const READY = { seen: false, last_stop: { reason: "end_turn" as const, at: NOW - 60_000 } };

test("ranks: the draft, the hand, an answer ready, answering, the rest (a failure unseen included); snoozed and done need nothing", () => {
  expect(conversationRank(row("d", { thread: null }), NOW)).toBe(0);
  expect(conversationRank(row("1", { status: "needs_you" }), NOW)).toBe(1);
  expect(conversationRank(row("2", READY), NOW)).toBe(2);
  expect(
    conversationRank(row("3", { seen: false, last_stop: { reason: "failed", at: NOW } }), NOW),
  ).toBe(5);
  // a failure that is answering again is Working
  expect(
    conversationRank(
      row("3", { status: "running", seen: false, last_stop: { reason: "failed", at: NOW } }),
      NOW,
    ),
  ).toBe(4);
  expect(conversationRank(row("4", { status: "running" }), NOW)).toBe(4);
  expect(conversationRank(row("5", { status: { queued: { behind: uuid(1, 3) } } }), NOW)).toBe(4);
  expect(conversationRank(row("6"), NOW)).toBe(5);
  expect(conversationRank(row("7", { status: "needs_you", done: true }), NOW)).toBe(5);
  expect(conversationRank(row("8", { ...READY, snoozed_until: NOW + 1 }), NOW)).toBe(5);
});

test("the list's own order (newest first) holds within a rank", () => {
  const rows = [
    row("r1"),
    row("w2", { status: "running" }),
    row("a3", READY),
    row("r4"),
    row("h5", { status: "needs_you" }),
    row("a6", READY),
  ];
  expect(orderConversations(rows, NOW).map((r) => r.key)).toEqual([
    "h5",
    "a3",
    "a6",
    "w2",
    "r1",
    "r4",
  ]);
});

test("held, the rows keep their places and a new one joins at the foot; let go, they settle", () => {
  const order = new ConversationOrder();
  const first = [row("r1"), row("r2")];
  expect(order.of(first, NOW, false).map((r) => r.key)).toEqual(["r1", "r2"]);
  // r2's answer comes in, and r3 appears, under the pointer
  const next = [row("r1"), row("r2", READY), row("r3", { status: "running" })];
  expect(order.of(next, NOW, true).map((r) => r.key)).toEqual(["r1", "r2", "r3"]);
  expect(order.of(next, NOW, false).map((r) => r.key)).toEqual(["r2", "r3", "r1"]);
  // a draft keeps the top even while held
  const drafted = [row("d", { thread: null }), ...next];
  expect(order.of(drafted, NOW, true).map((r) => r.key)).toEqual(["d", "r2", "r3", "r1"]);
});

/** A's conversations: the open one (newest, read), an older one answering, an older one with an answer unseen, and the
 * oldest waiting on a permission. */
function withConversations() {
  const open = thread(20, A, { title: "Lease, first read", last_activity: NOW });
  const busy = thread(21, A, { title: "Rent review clause", status: "running" });
  const ready = thread(22, A, { title: "Notice period", ...READY });
  const asks = thread(23, A, { title: "Indemnity cap", status: "needs_you" });
  const lists = new Map<string, ThreadInfo[]>(LISTS);
  lists.set(A.matter, [open, busy, ready, asks]);
  const matters = (): MatterInfo[] => [
    detailed(A, { last_opened: NOW, status: "needs_permission" }),
  ];
  return { lists, matters, open, ready };
}

test("the open matter's conversations: Needs your permission, then Answer ready, then Answering., then the rest", async () => {
  vi.spyOn(document, "hasFocus").mockReturnValue(true);
  const { lists, matters } = withConversations();
  await mount({ lists, matters });
  expect(titles()).toEqual([
    "Indemnity cap",
    "Notice period",
    "Rent review clause",
    "Lease, first read",
  ]);
  // said plainly on the row, under its title
  const words = conversations().map(
    (c) => c.querySelector(".room-conv-ask, .room-conv-said")?.textContent ?? "",
  );
  // the room opened the conversation waiting on the lawyer: in view, its row says nothing
  expect(conversations()[0]?.getAttribute("aria-current")).toBe("true");
  expect(words).toEqual(["", "Answer ready", "Answering.", ""]);
});

test("opening an answer ready marks it seen, and it leaves Needs you once the pointer leaves", async () => {
  vi.spyOn(document, "hasFocus").mockReturnValue(true);
  const { lists, matters, ready } = withConversations();
  const transport = await mount({ lists, matters });
  await hover(true);
  const target = conversations().find((c) => c.textContent?.includes("Notice period"));
  await press(target?.querySelector<HTMLElement>("button") ?? target);
  const marks = transport
    .only("next_command")
    .map((c) => c.command)
    .filter((c) => "mark_seen" in c);
  expect(marks).toContainEqual({ mark_seen: { thread: ready.thread } });
  // under the pointer nothing moves
  expect(titles()?.[1]).toBe("Notice period");
  await hover(false);
  // the conversation left now says what it waits for
  expect(
    conversations()
      .find((c) => c.textContent?.includes("Indemnity cap"))
      ?.querySelector(".room-conv-ask")?.textContent,
  ).toBe("Needs your permission");
  // seen, and open: it says nothing now, and stands with the rest, by its activity
  expect(titles()).toEqual([
    "Indemnity cap",
    "Rent review clause",
    "Lease, first read",
    "Notice period",
  ]);
});
