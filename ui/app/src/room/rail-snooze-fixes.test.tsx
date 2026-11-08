// The review's fixes to U16b's rail over FakeTransport: Undo and put-back give a thread back the snooze it had, a
// keyboard snooze keeps its line and its focus, the rail toggle's wait mark leaves out snoozed and done waits, a pinned
// matter snoozed whole stays in Pinned, the rail's clock starts at its mount, the picker and the menu send no time the
// host refuses, Mark all done's partial failure, and the open conversation's Snooze and Done behind its chevron.
// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot } from "react-dom/client";
import { settled } from "@clauseo/components/lib/late.ts";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { FakeTransport } from "../transport/fake-transport.ts";
import type { ThreadInfo } from "./host-next.ts";
import { LISTS, T_PIN, T_READY } from "./rail-needs-rig.ts";
import { railView } from "./rail-lists.ts";
import {
  A,
  arm,
  conversations,
  focusWindow,
  hover,
  needs,
  B,
  C,
  D,
  item,
  line,
  menuItems,
  mount,
  names,
  NOW,
  press,
  rail,
  sent,
  thread,
  T_A,
  T_ASK,
  unmount,
} from "./rail-snooze-rig.ts";
import { useRailClock } from "./sweep.ts";

beforeEach(() => {
  railView.set("matters");
  vi.useFakeTimers({ toFake: ["Date"] });
  vi.setSystemTime(NOW);
});

afterEach(async () => {
  await unmount();
  vi.useRealTimers();
});

const MONDAY = Date.UTC(2026, 10, 9, 9, 0);
const TOMORROW = Date.UTC(2026, 10, 4, 9, 0);
// the newest id opens (reopen.ts): List of dates
const T_LIST = thread(20, A, { title: "List of dates" });
const T_ARCHIVED = thread(7, A, { title: "Old draft", status: "archived" });
const T_OLD = thread(8, A, { title: "Chronology from the FIR", done: true });
const withA = (threads: ThreadInfo[]) => new Map(LISTS).set(A.matter, threads);
const actions = (name: string) =>
  rail()?.querySelector<HTMLElement>(`[aria-label="Actions for ${name}"]`);
const refuse = (message: string) => {
  throw { code: "command.invalid", message, retryable: false };
};
const quiet = () => vi.spyOn(console, "warn").mockImplementation(() => {});

test("Undo of a matter's snooze gives each thread back the snooze it had, and wakes the rest", async () => {
  const snoozed = { ...T_A, snoozed_until: MONDAY };
  const transport = await mount({ lists: withA([snoozed, T_LIST, T_ARCHIVED, T_OLD]) });
  await press(actions(A.name));
  // the submenu's row says Snooze until, its chevron saying it opens: no "…"
  expect(item("Snooze until")?.textContent).toBe("Snooze until");
  await press(item("Snooze until"));
  await press(item("Tomorrow 9:00 am"));
  // not the archived one, nor the done one; the one snoozed already, snoozed again
  expect(sent(transport, "snooze_thread")).toEqual([
    { thread: T_A.thread, until: TOMORROW },
    { thread: T_LIST.thread, until: TOMORROW },
  ]);
  await press(line()?.querySelector("button"));
  expect(sent(transport, "snooze_thread").slice(2)).toEqual([
    { thread: T_A.thread, until: MONDAY },
    { thread: T_LIST.thread },
  ]);
});

test("a snoozed row snoozed again and undone goes back to its earlier time; one gone by goes back to none", async () => {
  const transport = await mount({ lists: withA([{ ...T_A, snoozed_until: MONDAY }, T_LIST]) });
  const row = await arm("Bail conditions");
  await press(row?.querySelector('[data-testid="snooze"]'));
  await press(item("Tomorrow 9:00 am"));
  // the earlier time has gone by when Undo is pressed: no time
  vi.setSystemTime(MONDAY);
  await press(line()?.querySelector("button"));
  expect(sent(transport, "snooze_thread")).toEqual([
    { thread: T_A.thread, until: TOMORROW },
    { thread: T_A.thread },
  ]);
});

test("one thread refused: the one taken is put back to its own earlier snooze", async () => {
  const transport = await mount({
    lists: withA([{ ...T_A, snoozed_until: MONDAY }, T_LIST]),
    dispatch: (c) => {
      const s = (c.command as { snooze_thread?: { thread: string; until?: number } }).snooze_thread;
      return s?.thread === T_LIST.thread && s.until === TOMORROW ? refuse("past") : "done";
    },
  });
  const warn = quiet();
  await press(actions(A.name));
  await press(item("Snooze until"));
  await press(item("Tomorrow 9:00 am"));
  warn.mockRestore();
  expect(sent(transport, "snooze_thread").slice(2)).toEqual([
    { thread: T_A.thread, until: MONDAY },
  ]);
  expect(line()).toBeNull();
});

test("Mark all done with one refused: the one marked is marked not done again, and no line", async () => {
  const transport = await mount({
    lists: withA([T_A, T_LIST]),
    dispatch: (c) => {
      const d = (c.command as { mark_thread_done?: { thread: string; done: boolean } })
        .mark_thread_done;
      return d?.thread === T_LIST.thread && d.done ? refuse("busy") : "done";
    },
  });
  const warn = quiet();
  await press(actions(A.name));
  await press(item("Mark all done"));
  warn.mockRestore();
  expect(sent(transport, "mark_thread_done")).toEqual([
    { thread: T_A.thread, done: true },
    { thread: T_LIST.thread, done: true },
    { thread: T_A.thread, done: false },
  ]);
  expect(line()).toBeNull();
});

test("a keyboard snooze of a matter that isn't open keeps its line in place, with the focus on Undo", async () => {
  const transport = await mount();
  const row = [...(rail()?.querySelectorAll<HTMLElement>('[data-testid="matter-row"]') ?? [])].find(
    (r) => r.textContent?.includes(C.name),
  );
  // no pointer on the rail: the focus on C's row, its menu from the context key
  await act(async () => row?.focus());
  await act(async () =>
    row?.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true })),
  );
  await press(item("Snooze until"));
  await press(item("Tomorrow 9:00 am"));
  expect(line()).not.toBeNull();
  // the host's word comes: C, snoozed whole, would go to the shelf, but holds while its line stands
  await act(async () => {
    transport.appEvent({
      threads_changed: {
        matter: C.matter,
        threads: [{ ...T_READY, snoozed_until: TOMORROW }],
        removed: [],
        all: false,
      },
    });
  });
  await act(async () => settled());
  expect(line()).not.toBeNull();
  expect(document.activeElement).toBe(line()?.querySelector("button"));
});

const toggleWaits = () =>
  document.querySelector('[data-testid="rail-toggle"]')?.querySelector(".room-rail-wait") ?? null;

test("the rail toggle's yellow mark leaves out a snoozed or done wait", async () => {
  const lists = new Map(LISTS)
    .set(B.matter, [{ ...T_ASK, snoozed_until: NOW + 3_600_000 }])
    .set(D.matter, [{ ...T_PIN, done: true }]);
  await mount({ lists });
  expect(document.querySelector('[data-testid="rail-toggle"]')).not.toBeNull();
  expect(toggleWaits()).toBeNull();
  await unmount();
  // B's question awake: the mark
  await mount({ lists: new Map(lists).set(B.matter, [T_ASK]) });
  expect(toggleWaits()).not.toBeNull();
});

test("a pinned matter snoozed whole stays in Pinned, says so with the alarm clock, and isn't on the shelf", async () => {
  await mount({ lists: new Map(LISTS).set(D.matter, [{ ...T_PIN, snoozed_until: TOMORROW }]) });
  const pinned = [...(rail()?.querySelectorAll("section") ?? [])].find(
    (s) => s.querySelector(".room-when")?.textContent === "Pinned",
  );
  expect(names(pinned)).toEqual([D.name]);
  expect(pinned?.textContent).toContain("Snoozed until tomorrow 9:00\u00a0am");
  expect(pinned?.querySelector(".room-matter-mark")).not.toBeNull();
  expect(rail()?.querySelector('[data-testid="rail-shelf"]')).toBeNull();
});

test("the rail's clock starts at its own mount", async () => {
  const Clock = () => {
    const [stamp] = useRailClock(new FakeTransport(), "UTC");
    return createElement("output", null, String(stamp));
  };
  vi.setSystemTime(NOW + 3_600_000);
  const host = document.createElement("div");
  const root = createRoot(host);
  await act(async () => root.render(createElement(Clock)));
  expect(host.textContent).toBe(String(NOW + 3_600_000));
  await act(async () => root.unmount());
});

test("the picker can't confirm a time gone by, nor one past 366 days: Snooze stands greyed and sends nothing", async () => {
  const transport = await mount({ lists: withA([T_A, T_LIST]) });
  const row = await arm("Bail conditions");
  await press(row?.querySelector('[data-testid="snooze"]'));
  await press(item("Choose a day and time"));
  const field = rail()?.querySelector<HTMLInputElement>('[data-testid="snooze-picker"]');
  const submit = field?.form?.querySelector<HTMLButtonElement>('button[type="submit"]');
  for (const value of ["", "2026-11-03T16:59", "2027-11-05T10:00"]) {
    await act(async () => {
      if (field == null) return;
      Reflect.set(HTMLInputElement.prototype, "value", value, field);
      field.dispatchEvent(new Event("input", { bubbles: true }));
    });
    expect(submit?.disabled).toBe(true);
    await act(async () => field?.form?.requestSubmit());
  }
  expect(sent(transport, "snooze_thread")).toEqual([]);
  expect(rail()?.querySelector('[data-testid="snooze-picker"]')).not.toBeNull();
});

test("the menu's times are worked out as it opens; one gone by while it stood open sends nothing", async () => {
  vi.setSystemTime(NOW - 60_000);
  const transport = await mount({ lists: withA([T_A, T_LIST]) });
  const open = async () => {
    const row = await arm("Bail conditions");
    await press(row?.querySelector('[data-testid="snooze"]'));
    return menuItems().map((i) => i.textContent);
  };
  expect(await open()).toContain("6:00 pm");
  await act(async () =>
    document.activeElement?.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Escape", bubbles: true }),
    ),
  );
  await act(async () => settled());
  vi.setSystemTime(NOW);
  expect(await open()).toContain("9:00 pm");
  // left open past 9:00 pm, then 9:00 pm chosen
  vi.setSystemTime(Date.UTC(2026, 10, 3, 21, 0, 1));
  await press(item("9:00 pm"));
  expect(sent(transport, "snooze_thread")).toEqual([]);
  expect(line()).toBeNull();
});

test("the open conversation's chevron holds Snooze and Mark as done; once done, it ends in the tick and offers Mark as not done", async () => {
  const transport = await mount({ lists: withA([T_A, T_LIST]) });
  const li = await arm("List of dates");
  await press(li?.querySelector(`[aria-label="Actions for List of dates"]`));
  expect(menuItems().map((i) => i.textContent)).toEqual(["Snooze", "Mark as done"]);
  await press(item("Mark as done"));
  expect(sent(transport, "mark_thread_done")).toEqual([{ thread: T_LIST.thread, done: true }]);
  await unmount();
  const done = await mount({ lists: withA([T_A, { ...T_LIST, done: true }]) });
  const open = await arm("List of dates");
  expect(open?.querySelector(".room-conv .room-conv-mark")).not.toBeNull();
  expect(open?.querySelector(".room-conv .sr-only")?.textContent).toBe("Done");
  await press(open?.querySelector(`[aria-label="Actions for List of dates"]`));
  await press(item("Mark as not done"));
  expect(sent(done, "mark_thread_done")).toEqual([{ thread: T_LIST.thread, done: false }]);
});

test("a conversation's context menu, opened where it was asked for, holds Snooze and Mark as done", async () => {
  const transport = await mount({ lists: withA([T_A, T_LIST]) });
  const row = (await arm("Bail conditions"))?.querySelector<HTMLElement>(".room-conv");
  const asked = new MouseEvent("contextmenu", {
    bubbles: true,
    cancelable: true,
    clientX: 40,
    clientY: 300,
  });
  await act(async () => row?.dispatchEvent(asked));
  await act(async () => settled());
  expect(asked.defaultPrevented).toBe(true);
  expect(menuItems().map((i) => i.textContent)).toEqual(["Snooze", "Mark as done"]);
  await press(item("Mark as done"));
  expect(sent(transport, "mark_thread_done")).toEqual([{ thread: T_A.thread, done: true }]);
  expect(menuItems()).toEqual([]);
});

test("the open conversation's chevron is drawn from the first render, its menu loaded when pressed", async () => {
  await mount({ lists: withA([T_A, T_LIST]) });
  // no pointer, no focus: the chevron already stands, so the title's room never changes under the pointer
  const chevron = actions("List of dates");
  expect(chevron).not.toBeNull();
  // a row with Snooze is marked from the first render, before its buttons mount; the open one draws its chevron
  const bail = [...(rail()?.querySelectorAll(".room-conv-item") ?? [])].find((li) =>
    li.textContent?.includes("Bail conditions"),
  );
  expect(bail?.hasAttribute("data-triage")).toBe(true);
  expect(bail?.querySelector(".room-conv-acts")).toBeNull();
  expect(chevron?.closest(".room-conv-item")?.hasAttribute("data-triage")).toBe(false);
  expect(chevron?.closest(".room-conv-item")?.querySelector(".room-conv-more")).toBe(chevron);
  await press(chevron);
  expect(menuItems().map((i) => i.textContent)).toEqual(["Snooze", "Mark as done"]);
});

test("a row's two buttons mount with the focus on it, and Tab reaches them next, Snooze then Mark as done", async () => {
  await mount({ lists: withA([T_A, T_LIST]) });
  const row = conversations().find(
    (c) => c.querySelector(".room-conv-title")?.textContent === "Bail conditions",
  );
  await act(async () => row?.focus());
  await act(async () => settled());
  const li = row?.closest("li");
  const snooze = li?.querySelector<HTMLElement>('[data-testid="snooze"]');
  const done = li?.querySelector<HTMLElement>('[data-testid="mark-done"]');
  expect(snooze?.getAttribute("aria-label")).toBe("Snooze, Bail conditions");
  expect(done?.getAttribute("aria-label")).toBe("Mark as done, Bail conditions");
  // the tab order: every focusable in the rail, in the document's order, none taken out of it
  const order = [
    ...(rail()?.querySelectorAll<HTMLElement>("button, [href], [tabindex]") ?? []),
  ].filter((e) => e.tabIndex >= 0 && !e.hasAttribute("disabled"));
  const at = order.indexOf(row as HTMLElement);
  expect(at).toBeGreaterThanOrEqual(0);
  expect(order.slice(at + 1, at + 3)).toEqual([snooze, done]);
});

test("the later chunk is warmed once, when the page is idle after the rail mounts", async () => {
  const asked: (() => void)[] = [];
  const idle = vi.fn((run: () => void) => asked.push(run));
  vi.stubGlobal("requestIdleCallback", idle);
  vi.stubGlobal("cancelIdleCallback", vi.fn());
  try {
    await mount();
    expect(idle).toHaveBeenCalledTimes(1);
    await act(async () => asked[0]?.());
    await act(async () => settled());
    // nothing asks again: it fired and is gone
    await hover(true);
    await hover(false);
    expect(idle).toHaveBeenCalledTimes(1);
    await unmount();
  } finally {
    vi.unstubAllGlobals();
  }
});

test("a line that arrives once the pointer and the focus have left the rail never shows, and the groups move on", async () => {
  let answer: (() => void) | null = null;
  const transport = await mount({
    dispatch: (c) =>
      "snooze_thread" in c.command
        ? new Promise<"done">((resolve) => (answer = () => resolve("done")))
        : "done",
  });
  await hover(true);
  const row = [
    ...(needs()?.querySelectorAll<HTMLElement>('[data-testid="matter-row"]') ?? []),
  ].find((r) => r.textContent?.includes(C.name));
  await act(async () =>
    row?.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true })),
  );
  await press(item("Snooze until"));
  await press(item("Tomorrow 9:00 am"));
  await hover(false);
  // the host answers only now
  await act(async () => answer?.());
  await act(async () => settled());
  expect(line()).toBeNull();
  await act(async () => {
    transport.appEvent({
      threads_changed: {
        matter: C.matter,
        threads: [{ ...T_READY, snoozed_until: TOMORROW }],
        removed: [],
        all: false,
      },
    });
  });
  expect(names(needs())).not.toContain(C.name);
});

test("the window coming back clears a line", async () => {
  await mount({ lists: withA([T_A, T_LIST]) });
  await hover(true);
  const row = await arm("Bail conditions");
  await press(row?.querySelector('[data-testid="mark-done"]'));
  expect(line()).not.toBeNull();
  await focusWindow();
  expect(line()).toBeNull();
});

test("a row's context menu, its row losing Snooze while open, closes for good and doesn't come back with the row", async () => {
  const transport = await mount({ lists: withA([T_A, T_LIST]) });
  const row = (await arm("Bail conditions"))?.querySelector<HTMLElement>(".room-conv");
  await act(async () =>
    row?.dispatchEvent(
      new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: 40, clientY: 300 }),
    ),
  );
  await act(async () => settled());
  expect(menuItems().map((i) => i.textContent)).toEqual(["Snooze", "Mark as done"]);
  const change = (done: boolean) =>
    act(async () => {
      transport.appEvent({
        threads_changed: { matter: A.matter, threads: [{ ...T_A, done }], removed: [], all: false },
      });
    });
  // done elsewhere: the row goes to the fold, the menu with it
  await change(true);
  expect(menuItems()).toEqual([]);
  // not done again: the row comes back, the menu doesn't
  await change(false);
  await act(async () => settled());
  expect(menuItems()).toEqual([]);
});
