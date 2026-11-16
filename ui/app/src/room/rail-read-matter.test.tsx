// The open conversation's Let this conversation read (rail-triage-items.tsx), over FakeTransport: its submenu lists the
// other folder matters by name, never its own matter nor the scratch matter, and a press sends one allow_matter_read
// for the open conversation's thread. The item is reached from the open row's chevron and from its context menu
// (a right-click on the row), both tested here. A conversation not open offers no such item:
// rail-snooze-fixes.test.tsx's context menu test holds its items to Snooze and Mark as done.
// @vitest-environment happy-dom
import { act } from "react";
import { settled } from "@clauseo/components/lib/late.ts";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import type { ThreadInfo } from "./host-next.ts";
import { LISTS } from "./rail-needs-rig.ts";
import { railView } from "./rail-lists.ts";
import { readableOthers } from "./read-matters.ts";
import {
  A,
  arm,
  B,
  C,
  D,
  item,
  menuItems,
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

// the newest id opens (reopen.ts): List of dates
const T_LIST = thread(20, A, { title: "List of dates" });
const withA = (threads: ThreadInfo[]) => new Map(LISTS).set(A.matter, threads);
const actions = (name: string) =>
  rail()?.querySelector<HTMLElement>(`[aria-label="Actions for ${name}"]`);

test("the open conversation's menu lists the other matters by name, and a press lets it read that matter's files", async () => {
  const transport = await mount({ lists: withA([T_A, T_LIST]) });
  await press(actions("List of dates"));
  expect(menuItems().map((i) => i.textContent)).toEqual([
    "Snooze",
    "Mark as done",
    "Let this conversation read",
  ]);
  await press(item("Let this conversation read"));
  const names = menuItems()
    .map((i) => i.textContent)
    .slice(3);
  expect(names).toEqual([B.name, C.name, D.name]);
  await press(item(C.name));
  expect(sent(transport, "allow_matter_read")).toEqual([
    { thread: T_LIST.thread, matter: C.matter },
  ]);
});

test("a right-click on the open conversation's row opens the same menu: Snooze, Mark as done, Let this conversation read", async () => {
  await mount({ lists: withA([T_A, T_LIST]) });
  const row = (await arm("List of dates"))?.querySelector<HTMLElement>(".room-conv");
  expect(row).not.toBeNull();
  const asked = new MouseEvent("contextmenu", {
    bubbles: true,
    cancelable: true,
    clientX: 40,
    clientY: 300,
  });
  await act(async () => row?.dispatchEvent(asked));
  await act(async () => settled());
  expect(asked.defaultPrevented).toBe(true);
  expect(menuItems().map((i) => i.textContent)).toEqual([
    "Snooze",
    "Mark as done",
    "Let this conversation read",
  ]);
});

test("the matters offered leave out the conversation's own and those it may read already", () => {
  const names = new Map([
    [A.matter, A.name],
    [B.matter, B.name],
    [C.matter, C.name],
  ]);
  expect(readableOthers(names, A.matter, [C.matter])).toEqual([{ matter: B.matter, name: B.name }]);
});
