// The rail triage's matter fixes over FakeTransport (D-051, D-053, D-076): a matter's Mark all conversations done and
// Snooze until leave its row as it was (its name, its Actions, its press), with the line and its Undo beneath it for
// BRIEF_MS at most; a matter snoozed says so on its row once the line has gone; and Choose a day and time opens at
// 9:00 am tomorrow, so Snooze can be pressed at once.
// @vitest-environment happy-dom
import { act } from "react";
import { settled } from "@clauseo/components/lib/late.ts";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { LISTS } from "./rail-needs-rig.ts";
import { railView } from "./rail-lists.ts";
import { BRIEF_MS } from "./rail-snooze.tsx";
import {
  A,
  hover,
  item,
  line,
  mount,
  NOW,
  press,
  rail,
  sent,
  thread,
  T_A,
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

const T_LIST = thread(20, A, { title: "List of dates" });
const lists = () => new Map(LISTS).set(A.matter, [T_A, T_LIST]);
const actions = () => rail()?.querySelector<HTMLElement>(`[aria-label="Actions for ${A.name}"]`);
const nameOf = () => rail()?.querySelector(".room-house .room-matter-name")?.textContent;
const rowButton = () => rail()?.querySelector<HTMLElement>(".room-house .room-matter-row");

/** The pointer over the rail as the lawyer presses, so the line shows (rail-snooze.tsx's show). */
async function overRail() {
  await act(async () => rail()?.dispatchEvent(new PointerEvent("pointerover", { bubbles: true })));
}

/** BRIEF_MS go by on the faked clock. */
async function brief() {
  await act(async () => vi.advanceTimersByTime(BRIEF_MS));
  await act(async () => settled());
}

test("D-051: Mark all conversations done leaves the matter's row as it was, its line and Undo beneath it, briefly", async () => {
  const transport = await mount({ lists: lists() });
  vi.useFakeTimers({
    toFake: ["Date", "setTimeout", "clearTimeout"],
    now: NOW,
    shouldAdvanceTime: true,
  });
  await overRail();
  await press(actions());
  // the item says what it marks
  expect(item("Mark all done")).toBeUndefined();
  await press(item("Mark all conversations done"));
  expect(sent(transport, "mark_thread_done")).toEqual([
    { thread: T_A.thread, done: true },
    { thread: T_LIST.thread, done: true },
  ]);
  // the row keeps its name, its Actions and its press; the line stands beneath it
  expect(nameOf()).toBe(A.name);
  expect(actions()).not.toBeNull();
  expect(rowButton()).not.toBeNull();
  expect(line()?.textContent).toBe("All conversations marked as done Undo");
  expect(rowButton()?.contains(line())).toBe(false);
  // the pointer leaving the rail (the menu stood past its edge) leaves the line; it goes after BRIEF_MS, and the row stays
  await hover(false);
  expect(line()?.textContent).toBe("All conversations marked as done Undo");
  await brief();
  expect(line()).toBeNull();
  expect(nameOf()).toBe(A.name);
  expect(actions()).not.toBeNull();
});

test("D-051: the line's Undo marks the matter's conversations not done again", async () => {
  const transport = await mount({ lists: lists() });
  await overRail();
  await press(actions());
  await press(item("Mark all conversations done"));
  await press(line()?.querySelector("button"));
  expect(sent(transport, "mark_thread_done").slice(2)).toEqual([
    { thread: T_A.thread, done: false },
    { thread: T_LIST.thread, done: false },
  ]);
  expect(line()).toBeNull();
  expect(nameOf()).toBe(A.name);
});

test("D-053: Snooze until leaves the row with its name; the line goes after BRIEF_MS, and the row says it's snoozed", async () => {
  const until = Date.UTC(2026, 10, 4, 9, 0);
  let snoozed = false;
  const transport = await mount({
    lists: lists(),
    dispatch: (command) => {
      if (command.command.snooze_thread !== undefined) snoozed = true;
      return "done";
    },
  });
  vi.useFakeTimers({
    toFake: ["Date", "setTimeout", "clearTimeout"],
    now: NOW,
    shouldAdvanceTime: true,
  });
  await overRail();
  await press(actions());
  await press(item("Snooze until"));
  await press(item("Tomorrow 9:00 am"));
  expect(snoozed).toBe(true);
  expect(sent(transport, "snooze_thread")).toEqual([
    { thread: T_A.thread, until },
    { thread: T_LIST.thread, until },
  ]);
  expect(nameOf()).toBe(A.name);
  expect(actions()).not.toBeNull();
  expect(line()?.textContent).toBe("Snoozed until tomorrow 9:00\u00a0am Undo");
  // the host's list says both are snoozed; the line says it once, then the row's own snoozed look
  await act(async () =>
    transport.appEvent({
      threads_changed: {
        matter: A.matter,
        threads: [T_A, T_LIST].map((t) => ({ ...t, snoozed_until: until })),
        removed: [],
        all: true,
      },
    }),
  );
  await act(async () => settled());
  // the line says it, and the row itself doesn't say it again while the line stands
  expect(line()?.textContent).toContain("Snoozed until");
  expect(rowButton()?.textContent).not.toContain("Snoozed until");
  await brief();
  expect(line()).toBeNull();
  expect(nameOf()).toBe(A.name);
  expect(rail()?.querySelector(".room-house .room-matter-row")?.textContent).toContain(
    "Snoozed until tomorrow 9:00\u00a0am",
  );
});

test("D-076: Choose a day and time opens at 9:00 am tomorrow, Snooze ready to press", async () => {
  const transport = await mount({ lists: lists() });
  await press(actions());
  await press(item("Snooze until"));
  await press(item("Choose a day and time"));
  const field = rail()?.querySelector<HTMLInputElement>('[data-testid="snooze-picker"]');
  // NOW is 5:00 pm on 3 November in the test's zone: tomorrow at 9:00
  expect(field?.value).toBe("2026-11-04T09:00");
  const snooze = field?.form?.querySelector<HTMLButtonElement>('button[type="submit"]');
  expect(snooze?.disabled).toBe(false);
  await act(async () => field?.form?.requestSubmit());
  await act(async () => settled());
  const until = Date.UTC(2026, 10, 4, 9, 0);
  expect(sent(transport, "snooze_thread")).toEqual([
    { thread: T_A.thread, until },
    { thread: T_LIST.thread, until },
  ]);
});
