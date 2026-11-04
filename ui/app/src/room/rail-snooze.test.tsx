// U16b's snooze on the rail over FakeTransport, at the room's interface: the morning sweep at launch and when the window
// comes back (never before 05:00), a snoozed row back at the first render after its time and not before, the wake, the
// menu's times, a refused snooze leaving nothing changed, and Undo.
// @vitest-environment happy-dom
import { act } from "react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import type { MatterInfo, ThreadInfo } from "./host-next.ts";
import { LISTS, MATTERS } from "./rail-needs-rig.ts";
import { railView } from "./rail-lists.ts";
import {
  A,
  arm,
  B,
  C,
  conversations,
  focusWindow,
  hover,
  item,
  line,
  menuItems,
  mount,
  names,
  needs,
  NOW,
  press,
  rail,
  sent,
  thread,
  titles,
  T_A,
  T_ASK,
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
});

/** Today's midnight in the suite's zone (UTC). */
const MIDNIGHT = Date.UTC(2026, 10, 3);
const HOUR = 3_600_000;
const T_LIST = thread(10, A, { title: "List of dates" });
const withA = (more: ThreadInfo[], next?: string) => {
  const lists = new Map(LISTS);
  lists.set(A.matter, [...(lists.get(A.matter) ?? []), ...more]);
  const matters =
    next === undefined
      ? MATTERS
      : () =>
          MATTERS().map((m): MatterInfo => (m.matter === A.matter ? { ...m, next_date: next } : m));
  return { lists, matters };
};

/** The snooze menu of Bail conditions (List of dates is the open one), open: its items' words. */
async function menuOf() {
  const row = await arm("Bail conditions");
  await press(row?.querySelector<HTMLElement>('[data-testid="snooze"]'));
  return menuItems().map((i) => i.textContent);
}

test("sweep_done goes at launch with today's local midnight, and again each time the window comes back", async () => {
  const transport = await mount();
  expect(sent(transport, "sweep_done")).toEqual([{ day_start: MIDNIGHT }]);
  await focusWindow();
  expect(sent(transport, "sweep_done")).toEqual([{ day_start: MIDNIGHT }, { day_start: MIDNIGHT }]);
  await act(async () => document.dispatchEvent(new Event("visibilitychange")));
  expect(sent(transport, "sweep_done")).toHaveLength(3);
  // a control taking the focus inside the window is no return
  await act(async () => rail()?.querySelector("button")?.focus());
  expect(sent(transport, "sweep_done")).toHaveLength(3);
});

test("before 05:00 nothing is swept, at launch or on focus; the first focus from 05:00 sweeps", async () => {
  vi.setSystemTime(MIDNIGHT + 5 * HOUR - 60_000);
  const transport = await mount();
  await focusWindow();
  expect(sent(transport, "sweep_done")).toEqual([]);
  vi.setSystemTime(MIDNIGHT + 5 * HOUR);
  await focusWindow();
  expect(sent(transport, "sweep_done")).toEqual([{ day_start: MIDNIGHT }]);
});

test("a snoozed row comes back at the first render after its time, and not before", async () => {
  const lists = new Map(LISTS);
  lists.set(B.matter, [{ ...T_ASK, snoozed_until: NOW + HOUR }]);
  await mount({ lists });
  // B's one conversation snoozed: the matter is snoozed whole, on the shelf, out of Needs you
  expect(names(needs())).toEqual([C.name]);
  const shelf = rail()?.querySelector('[data-testid="rail-shelf"]');
  expect(shelf?.querySelector(".room-fold-count")?.textContent).toBe("1");
  expect(shelf?.textContent).toContain("Next back 6:00\u00a0pm");
  vi.setSystemTime(NOW + HOUR - 1);
  await focusWindow();
  expect(names(needs())).toEqual([C.name]);
  // the time comes with nothing drawn: nothing changes until the next render
  vi.setSystemTime(NOW + HOUR);
  expect(names(needs())).toEqual([C.name]);
  await focusWindow();
  expect(names(needs())).toEqual([B.name, C.name]);
  expect(rail()?.querySelector('[data-testid="rail-shelf"]')).toBeNull();
});

test("the wake: the reducer clearing a snooze brings the row back to Needs you with its status", async () => {
  const lists = new Map(LISTS);
  lists.set(B.matter, [{ ...T_ASK, snoozed_until: NOW + 5 * HOUR }, thread(11, B)]);
  const transport = await mount({ lists });
  expect(names(needs())).toEqual([C.name]);
  await act(async () => {
    transport.appEvent({
      threads_changed: { matter: B.matter, threads: [T_ASK], removed: [], all: false },
    });
  });
  expect(names(needs())).toEqual([B.name, C.name]);
  const row = needs()?.querySelector('[data-testid="matter-row"]');
  expect(row?.getAttribute("data-state")).toBe("permission");
  expect(row?.querySelector(".room-matter-where")?.textContent).toBe(T_ASK.title);
});

test("the menu at 4:59 pm offers 6:00 pm, and at 5:00 pm 9:00 pm in its place", async () => {
  vi.setSystemTime(NOW - 60_000);
  const { lists } = withA([T_LIST]);
  await mount({ lists });
  expect(await menuOf()).toEqual([
    "6:00 pm",
    "Tomorrow 9:00 am",
    "Monday 9:00 am",
    "Choose a day and time",
  ]);
  await unmount();
  vi.setSystemTime(NOW);
  await mount({ lists });
  expect(await menuOf()).toEqual([
    "9:00 pm",
    "Tomorrow 9:00 am",
    "Monday 9:00 am",
    "Choose a day and time",
  ]);
});

test("Before the next date shows only with the matter's next date more than a day away", async () => {
  await mount(withA([T_LIST], "2026-11-11"));
  // 9:00 am the day before, its day and time beneath
  expect(await menuOf()).toContain("Before the next dateTuesday 10 November, 9:00 am");
  await unmount();
  await mount(withA([T_LIST], "2026-11-04"));
  expect((await menuOf()).some((i) => i?.startsWith("Before the next date"))).toBe(false);
});

test("a snooze: one snooze_thread with its time, the line in the row's place until the pointer leaves, then the row with its alarm clock", async () => {
  const { lists } = withA([T_LIST]);
  const transport = await mount({ lists });
  await hover(true);
  await menuOf();
  await press(item("Tomorrow 9:00 am"));
  const until = Date.UTC(2026, 10, 4, 9, 0);
  expect(sent(transport, "snooze_thread")).toEqual([{ thread: T_A.thread, until }]);
  expect(line()?.textContent).toBe("Snoozed until tomorrow 9:00\u00a0am Undo");
  expect(titles()).not.toContain("Bail conditions");
  await act(async () => {
    transport.appEvent({
      threads_changed: {
        matter: A.matter,
        threads: [{ ...T_A, snoozed_until: until }],
        removed: [],
        all: false,
      },
    });
  });
  expect(line()).not.toBeNull();
  await hover(false);
  expect(line()).toBeNull();
  const row = conversations().find((c) => c.textContent?.includes("Bail conditions"));
  expect(row?.querySelector(".room-conv-mark")).not.toBeNull();
  expect(row?.getAttribute("title")).toContain("Snoozed until tomorrow 9:00\u00a0am");
});

test("Undo sends snooze_thread with no time, and the row is back", async () => {
  const { lists } = withA([T_LIST]);
  const transport = await mount({ lists });
  await menuOf();
  await press(item("Tomorrow 9:00 am"));
  await press(line()?.querySelector("button"));
  const both = sent(transport, "snooze_thread");
  expect(both).toEqual([
    { thread: T_A.thread, until: Date.UTC(2026, 10, 4, 9, 0) },
    { thread: T_A.thread },
  ]);
  expect(Object.keys(both[1] ?? {})).toEqual(["thread"]);
  expect(line()).toBeNull();
  expect(titles()).toContain("Bail conditions");
});

// the host's own rule (MAX_SNOOZE_AHEAD_MS) on a clock of its own: two days ahead of the app's, Tomorrow 9:00 am is past
// for it; a year and a day behind, Monday 9:00 am is more than 366 days ahead
const CASES = [
  { why: "a time past", skew: 2 * 24 * HOUR, pick: "Tomorrow 9:00 am", refused: "not after now" },
  {
    why: "more than 366 days ahead",
    skew: -367 * 24 * HOUR,
    pick: "Monday 9:00 am",
    refused: "too far ahead",
  },
];
for (const { why, skew, pick, refused } of CASES)
  test(`snooze_thread refused (${why}): the conversation stays as it was, and no line`, async () => {
    const { lists } = withA([T_LIST]);
    const said: string[] = [];
    const transport = await mount({
      lists,
      dispatch: (command) => {
        const until = (command.command as { snooze_thread?: { until?: number } }).snooze_thread
          ?.until;
        if (until === undefined) return "done";
        const now = Date.now() + skew;
        const message =
          until <= now ? "not after now" : until - now > 366 * 24 * HOUR ? "too far ahead" : null;
        if (message === null) return "done";
        said.push(message);
        throw { code: "command.invalid", message, retryable: false };
      },
    });
    await menuOf();
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    await press(item(pick));
    warn.mockRestore();
    expect(said).toEqual([refused]);
    expect(sent(transport, "snooze_thread")).toHaveLength(1);
    expect(line()).toBeNull();
    expect(titles()).toContain("Bail conditions");
    const row = conversations().find((c) => c.textContent?.includes("Bail conditions"));
    expect(row?.querySelector(".room-conv-mark")).toBeNull();
  });
