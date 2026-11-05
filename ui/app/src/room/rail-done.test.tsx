// U16b's Done and matter snooze on the rail over FakeTransport: Mark as done and its Undo, the Done fold's count and
// Mark as not done, a matter snoozed (one snooze_thread per conversation not done) and Mark all done, the picker, the
// open shelf's Wake now, and axe over the new parts in both themes.
// @vitest-environment happy-dom
import axe from "axe-core";
import { act } from "react";
import { settled } from "@clauseo/components/lib/late.ts";
import { afterEach, beforeAll, beforeEach, expect, test, vi } from "vitest";
import { LISTS } from "./rail-needs-rig.ts";
import { railView } from "./rail-lists.ts";
import {
  A,
  arm,
  B,
  hover,
  item,
  line,
  mount,
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

beforeAll(() => {
  document.title = "Clauseo";
});

beforeEach(() => {
  railView.set("matters");
  vi.useFakeTimers({ toFake: ["Date"] });
  vi.setSystemTime(NOW);
});

afterEach(async () => {
  await unmount();
  vi.useRealTimers();
});

// the newest id opens (reopen.ts): List of dates, so Bail conditions is a row with its buttons
const T_LIST = thread(20, A, { title: "List of dates" });
const T_OLD = thread(8, A, { title: "Chronology from the FIR", done: true });
const T_EARLIER = thread(9, A, { title: "Earlier bail order, 2024", done: true });
const lists = () => {
  const l = new Map(LISTS);
  l.set(A.matter, [T_A, T_LIST, T_OLD, T_EARLIER]);
  return l;
};
const fold = () => rail()?.querySelector<HTMLElement>('[data-testid="done-fold"]') ?? null;

test("the Done fold: its count, its rows once open, and Mark as not done", async () => {
  const transport = await mount({ lists: lists() });
  expect(titles()).toEqual(["Bail conditions", "List of dates"]);
  expect(fold()?.querySelector(".room-fold")?.textContent).toBe("Done2");
  expect(fold()?.querySelector(".room-fold")?.getAttribute("aria-expanded")).toBe("false");
  // a fold's chevron, 13 px (DESIGN.md)
  expect(fold()?.querySelector(".room-fold-icon")?.getAttribute("width")).toBe("13");
  await press(fold()?.querySelector(".room-fold"));
  const rows = [...(fold()?.querySelectorAll('[data-testid="done-row"]') ?? [])];
  expect(rows.map((r) => r.querySelector(".room-conv-title")?.textContent)).toEqual([
    "Chronology from the FIR",
    "Earlier bail order, 2024",
  ]);
  expect(fold()?.querySelector(".room-fold-head .room-info")?.getAttribute("aria-label")).toBe(
    "Conversations you've read, with nothing waiting, move here each morning.",
  );
  await press(rows[0]?.querySelector('[data-testid="mark-not-done"]'));
  expect(sent(transport, "mark_thread_done")).toEqual([{ thread: T_OLD.thread, done: false }]);
});

test("Mark as done: mark_thread_done, the line in the row's place, and Undo sends done false", async () => {
  const transport = await mount({ lists: lists() });
  const row = await arm("Bail conditions");
  await press(row?.querySelector('[data-testid="mark-done"]'));
  expect(sent(transport, "mark_thread_done")).toEqual([{ thread: T_A.thread, done: true }]);
  expect(line()?.textContent).toBe("Marked as done Undo");
  await press(line()?.querySelector("button"));
  expect(sent(transport, "mark_thread_done")).toEqual([
    { thread: T_A.thread, done: true },
    { thread: T_A.thread, done: false },
  ]);
  expect(line()).toBeNull();
});

test("a done row moves into the fold once the pointer leaves", async () => {
  const transport = await mount({ lists: lists() });
  await hover(true);
  const row = await arm("Bail conditions");
  await press(row?.querySelector('[data-testid="mark-done"]'));
  await act(async () => {
    transport.appEvent({
      threads_changed: {
        matter: A.matter,
        threads: [{ ...T_A, done: true }],
        removed: [],
        all: false,
      },
    });
  });
  expect(line()).not.toBeNull();
  await hover(false);
  expect(line()).toBeNull();
  expect(titles()).toEqual(["List of dates"]);
  expect(fold()?.querySelector(".room-fold-count")?.textContent).toBe("3");
});

test("a matter's Snooze until sends one snooze_thread per conversation not done; Mark all done one mark_thread_done each", async () => {
  const transport = await mount({ lists: lists() });
  await press(rail()?.querySelector(`[aria-label="Actions for ${A.name}"]`));
  await press(item("Snooze until"));
  await press(item("Tomorrow 9:00 am"));
  const until = Date.UTC(2026, 10, 4, 9, 0);
  expect(sent(transport, "snooze_thread")).toEqual([
    { thread: T_A.thread, until },
    { thread: T_LIST.thread, until },
  ]);
  expect(line()?.textContent).toBe("Snoozed until tomorrow 9:00\u00a0am Undo");
  await press(line()?.querySelector("button"));
  expect(sent(transport, "snooze_thread").slice(2)).toEqual([
    { thread: T_A.thread },
    { thread: T_LIST.thread },
  ]);
  await press(rail()?.querySelector(`[aria-label="Actions for ${A.name}"]`));
  await press(item("Mark all done"));
  expect(sent(transport, "mark_thread_done")).toEqual([
    { thread: T_A.thread, done: true },
    { thread: T_LIST.thread, done: true },
  ]);
});

test("one thread of a matter refused: those taken are put back, and no line", async () => {
  const transport = await mount({
    lists: lists(),
    dispatch: (command) => {
      const s = (command.command as { snooze_thread?: { thread: string; until?: number } })
        .snooze_thread;
      if (s?.thread === T_LIST.thread && s.until !== undefined)
        throw { code: "command.invalid", message: "past", retryable: false };
      return "done";
    },
  });
  const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
  await press(rail()?.querySelector(`[aria-label="Actions for ${A.name}"]`));
  await press(item("Snooze until"));
  await press(item("Monday 9:00 am"));
  warn.mockRestore();
  expect(sent(transport, "snooze_thread").slice(2)).toEqual([{ thread: T_A.thread }]);
  expect(line()).toBeNull();
});

test("Choose a day and time: the picker in the row's place sends its moment; Escape leaves it", async () => {
  const transport = await mount({ lists: lists() });
  const row = await arm("Bail conditions");
  await press(row?.querySelector('[data-testid="snooze"]'));
  await press(item("Choose a day and time"));
  const field = rail()?.querySelector<HTMLInputElement>('[data-testid="snooze-picker"]');
  expect(field?.min).toBe("2026-11-03T17:01");
  await act(async () => {
    if (field == null) return;
    Reflect.set(HTMLInputElement.prototype, "value", "2026-11-06T10:30", field);
    field.dispatchEvent(new Event("input", { bubbles: true }));
  });
  await act(async () => field?.form?.requestSubmit());
  await act(async () => settled());
  expect(sent(transport, "snooze_thread")).toEqual([
    { thread: T_A.thread, until: Date.UTC(2026, 10, 6, 10, 30) },
  ]);
  expect(rail()?.querySelector('[data-testid="snooze-picker"]')).toBeNull();
});

test("the open shelf: a row with its matter and time, Wake now sending snooze_thread with no time", async () => {
  const l = lists();
  l.set(B.matter, [{ ...T_ASK, snoozed_until: NOW + 3_600_000 }, thread(13, B)]);
  const transport = await mount({ lists: l });
  const shelf = rail()?.querySelector<HTMLElement>('[data-testid="rail-shelf"]');
  await press(shelf?.querySelector(".room-fold"));
  const row = shelf?.querySelector('[data-testid="shelf-row"]');
  expect(row?.textContent).toBe(`${T_ASK.title}${B.name}Until 6:00\u00a0pm`);
  await press(row?.querySelector('[data-testid="wake-now"]'));
  expect(sent(transport, "snooze_thread")).toEqual([{ thread: T_ASK.thread }]);
});

test("an open fold's info icon stands in its heading's row after its name, outside the heading, for Done and the shelf", async () => {
  const l = lists();
  l.set(B.matter, [{ ...T_ASK, snoozed_until: NOW + 3_600_000 }]);
  await mount({ lists: l });
  const shelf = rail()?.querySelector<HTMLElement>('[data-testid="rail-shelf"]');
  for (const f of [fold(), shelf]) {
    // folded: the slot stands empty, so nothing moves as the icon comes
    expect(f?.querySelector(".room-fold-slot")?.childElementCount).toBe(0);
    await press(f?.querySelector(".room-fold"));
    const head = f?.querySelector(".room-fold-head");
    const info = head?.querySelector(".room-info");
    expect(info).not.toBeNull();
    // beside the heading, never in its name, and nowhere under the list
    expect(info?.closest("h1, h2, h3, h4, h5, h6, .room-fold")).toBeNull();
    expect(
      info?.parentElement?.previousElementSibling?.querySelector(".room-fold-name"),
    ).not.toBeNull();
    expect(f?.querySelectorAll(".room-info")).toHaveLength(1);
  }
  expect(fold()?.querySelector(".room-info")?.getAttribute("aria-label")).toBe(
    "Conversations you've read, with nothing waiting, move here each morning.",
  );
  expect(shelf?.querySelector(".room-info")?.getAttribute("aria-label")).toBe(
    "Back sooner if Clauseo needs you or an answer finishes.",
  );
});

const violations = async () =>
  (await axe.run(document, { resultTypes: ["violations"] })).violations.map(
    (v) => `${v.id} at ${v.nodes.map((n) => n.target.join(" ")).join(", ")}`,
  );

for (const theme of ["light", "dark"] as const)
  test(`axe finds no violation over snooze and Done: ${theme}`, async () => {
    const l = lists();
    l.set(B.matter, [{ ...T_ASK, snoozed_until: NOW + 3_600_000 }]);
    l.set(A.matter, [T_A, { ...T_LIST, snoozed_until: NOW + 7_200_000 }, T_OLD, T_EARLIER]);
    await mount({ lists: l, theme });
    expect(document.documentElement.dataset["theme"]).toBe(theme === "dark" ? "night" : "day");
    // the buttons under the pointer, the fold and the shelf open, a whole matter on the shelf
    await arm("Bail conditions");
    await press(fold()?.querySelector(".room-fold"));
    await press(rail()?.querySelector('[data-testid="rail-shelf"] .room-fold'));
    expect(rail()?.querySelector('[data-testid="snooze"]')).not.toBeNull();
    expect(rail()?.textContent).toContain("Whole matter");
    // Wake now and Mark as not done don't share an icon
    const glyph = (id: string) => rail()?.querySelector(`[data-testid="${id}"] svg`)?.innerHTML;
    expect(glyph("wake-now")).toBeDefined();
    expect(glyph("wake-now")).not.toBe(glyph("mark-not-done"));
    expect(await violations()).toEqual([]);
    // the line in a row's place
    await press(rail()?.querySelector('[data-testid="mark-done"]'));
    expect(line()).not.toBeNull();
    expect(await violations()).toEqual([]);
  }, 20_000);
