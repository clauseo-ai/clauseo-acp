// U16a's rail over FakeTransport, at the room's interface: Needs you under Pinned with the conversation behind each
// row, a press opening that conversation, rows holding their group under the pointer, the order rows come in, the open
// matter staying put, Recent's list, and nothing ticking or committing at rest with Recent open.
// @vitest-environment happy-dom
import { act } from "react";
import { railView } from "./rail-lists.ts";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import {
  A,
  B,
  C,
  D,
  headings,
  mount,
  namesIn,
  needs,
  NOW,
  rail,
  rowOf,
  settle,
  T_READY,
  theHost,
  thread,
  unmount,
} from "./rail-needs-rig.ts";

beforeEach(() => {
  // the switch holds its side for the session: each test starts on Matters
  railView.set("matters");
  vi.useFakeTimers({ toFake: ["Date"] });
  vi.setSystemTime(NOW);
});

afterEach(async () => {
  await unmount();
  vi.useRealTimers();
});

test("Needs you stands under Pinned, above the date groups, each row naming the conversation behind it", async () => {
  await mount();
  expect(headings()).toEqual(["Pinned", "Needs you", "No date yet"]);
  expect(needs()?.querySelector(".room-when")?.textContent).toBe("Needs you");
  // the hand's waits first, then the answer ready
  expect(namesIn(needs())).toEqual([B.name, C.name]);
  expect(rowOf(B.name)?.querySelector(".room-matter-where")?.textContent).toBe(
    "Reply to the notice",
  );
  expect(rowOf(B.name)?.textContent).toContain("Needs your permission");
  expect(rowOf(C.name)?.querySelector(".room-matter-where")?.textContent).toBe(
    "Counterclaim, limitation",
  );
  // a pinned matter that needs the lawyer stays in Pinned, its line and conversation with it
  const pinned = rail()?.querySelector(".room-groups > section");
  expect(namesIn(pinned)).toEqual([D.name]);
  expect(rowOf(D.name)?.textContent).toContain("A question for you");
  expect(rowOf(D.name)?.querySelector(".room-matter-where")?.textContent).toBe("Show cause reply");
});

test("a press on a Needs-you row opens that conversation in its matter", async () => {
  const transport = await mount();
  await act(async () => rowOf(B.name)?.click());
  await settle();
  expect(theHost()?.querySelector('[data-testid="matter"]')?.textContent).toBe(B.name);
  // the conversation behind the row is the one open, raised on the rail
  const open = rail()?.querySelector('[data-testid="conversation"][aria-current="true"]');
  expect(open?.querySelector(".room-conv-title")?.textContent).toBe("Reply to the notice");
  // the open matter stays in Needs you while open
  expect(namesIn(needs())).toContain(B.name);
  expect(transport.calls.some((c) => c.call === "next_command" && "open_matter" in c.command)).toBe(
    true,
  );
});

test("in the open matter's list, a conversation waiting on a permission says so, as Needs you does", async () => {
  await mount();
  await act(async () => rowOf(B.name)?.click());
  await settle();
  // a draft open, so the conversation's row says what it's doing
  await act(async () =>
    rail()?.querySelector<HTMLButtonElement>('[data-testid="new-conversation"]')?.click(),
  );
  await settle();
  const row = [
    ...(rail()?.querySelectorAll<HTMLElement>('[data-testid="conversation"]') ?? []),
  ].find((r) => r.querySelector(".room-conv-title")?.textContent === "Reply to the notice");
  expect(row?.dataset["state"]).toBe("permission");
  expect(row?.title).toContain("Needs your permission");
  expect(row?.textContent).not.toContain("A question for you");
});

test("no row changes group under the pointer; at pointer leave it does", async () => {
  const transport = await mount();
  const nav = rail();
  if (nav === null) throw new Error("no rail");
  act(() => {
    nav.dispatchEvent(new PointerEvent("pointerover", { bubbles: true }));
  });
  // C's answer is read elsewhere and B's wait answered while the pointer is over the rail
  await act(async () => {
    transport.appEvent({
      threads_changed: {
        matter: C.matter,
        threads: [{ ...T_READY, seen: true }],
        removed: [],
        all: false,
      },
    });
  });
  expect(namesIn(needs())).toEqual([B.name, C.name]);
  act(() => {
    nav.dispatchEvent(new PointerEvent("pointerout", { bubbles: true }));
  });
  expect(namesIn(needs())).toEqual([B.name]);
  expect(headings()).toEqual(["Pinned", "Needs you", "No date yet"]);
});

test("a row that comes to Needs you later joins its kind's foot", async () => {
  const transport = await mount();
  const asking = thread(6, C, { title: "Second question", status: "needs_you" });
  await act(async () => {
    transport.appEvent({
      threads_changed: { matter: C.matter, threads: [asking], removed: [], all: false },
    });
  });
  // C now waits on the hand too, and came after B
  expect(namesIn(needs())).toEqual([B.name, C.name]);
  expect(rowOf(C.name)?.querySelector(".room-matter-where")?.textContent).toBe("Second question");
  const a = thread(7, A, { title: "A late wait", status: "needs_you" });
  await act(async () => {
    transport.appEvent({
      threads_changed: { matter: A.matter, threads: [a], removed: [], all: false },
    });
  });
  // A is the open matter: it stays where it stood
  expect(namesIn(needs())).toEqual([B.name, C.name]);
});

test("Recent lists every conversation, scratch included, under its day; a press opens it", async () => {
  await mount();
  await act(async () =>
    rail()?.querySelector<HTMLButtonElement>('[data-testid="rail-switch-recent"]')?.click(),
  );
  const rows = [...(rail()?.querySelectorAll<HTMLElement>('[data-testid="recent-row"]') ?? [])];
  expect(rows.map((r) => r.querySelector(".room-conv-title")?.textContent)).toEqual([
    "Reply to the notice",
    "Counterclaim, limitation",
    "Show cause reply",
    "Bail conditions",
    "Cheque bounce by email",
  ]);
  expect(rows[4]?.querySelector(".room-matter-note")?.textContent).toBe("Scratch matter");
  expect(headings()).toEqual(["Today"]);
  // a conversation waiting on the lawyer takes its matter's kind: Sharma's a permission, as Needs you says it, with
  // its shield on the yellow disc and its words only in the tooltip; Desai's a question, whose words show
  expect(rows[0]?.dataset["state"]).toBe("permission");
  expect(rows[0]?.querySelector(".room-conv-ask")).toBeNull();
  expect(rows[0]?.title).toContain("Needs your permission");
  expect(rows[0]?.querySelector("[data-waits]")).not.toBeNull();
  expect(rows[2]?.querySelector(".room-conv-ask")?.textContent).toBe("A question for you");
  expect(rows[1]?.querySelector(".room-conv-ask")).toBeNull();
  expect(rows[1]?.textContent).toContain("Answer ready");
  await act(async () => rows[1]?.click());
  await settle();
  expect(theHost()?.querySelector('[data-testid="matter"]')?.textContent).toBe(C.name);
  // the switch stays on Recent, the opened conversation raised
  const raised = rail()?.querySelector('[data-testid="recent-row"][aria-current="true"]');
  expect(raised?.getAttribute("data-conversation")).toBe(T_READY.thread);
  // the switch hides while Find has the focus
  const find = rail()?.querySelector<HTMLInputElement>('[data-testid="find"]');
  await act(async () => find?.focus());
  expect(rail()?.querySelector('[data-testid="rail-switch"]')).toBeNull();
});

test("at rest with Recent open: no timer and no commit over 5 s", async () => {
  vi.useRealTimers();
  vi.useFakeTimers({ toFake: ["Date", "setTimeout", "setInterval", "requestAnimationFrame"] });
  vi.setSystemTime(NOW);
  const phases: string[] = [];
  await mount((phase) => phases.push(phase), false);
  await act(async () => {
    await vi.runAllTimersAsync();
    await vi.dynamicImportSettled();
  });
  await act(async () =>
    rail()?.querySelector<HTMLButtonElement>('[data-testid="rail-switch-recent"]')?.click(),
  );
  await act(async () => {
    await vi.runAllTimersAsync();
    await vi.dynamicImportSettled();
  });
  expect(rail()?.querySelector('[data-testid="recent-row"]')).not.toBeNull();
  expect(vi.getTimerCount()).toBe(0);
  phases.length = 0;
  await act(async () => vi.advanceTimersByTimeAsync(5_000));
  expect(phases).toEqual([]);
});
