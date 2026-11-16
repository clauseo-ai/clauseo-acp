// @vitest-environment happy-dom
// A Find result's jump to an entry (thread/jump-entry.ts) never outlives its open (U16a): paging back to the thread's
// start without the entry ends it, a result found by its title clears it, and so does opening another conversation,
// so a thread opened days later doesn't page back to an old entry.
import { act } from "react";
import { settled } from "@clauseo/components/lib/late.ts";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { streamOf } from "../store/view.ts";
import { agentText, MATTER, reset, THREAD, userMessage, uuid } from "../testing/stream-script.ts";
import type { HostQuery, QueryResult, SearchHit, SnippetPart } from "../transport/protocol.ts";
import { clearJump, jumpAsked, jumpTo, landing } from "./thread/jump-entry.ts";
import { HIT_MARGIN, nudge } from "./thread/jump-hit.ts";
import { mountRoom, summary, THE_MATTER, type Harness } from "./thread/harness.ts";

const NOW = Date.UTC(2026, 10, 3, 17, 0);
const T2 = uuid(2, 1);
const T4 = uuid(4, 1);

let h: Harness | null = null;
beforeEach(() => {
  vi.useFakeTimers({ toFake: ["Date"] });
  vi.setSystemTime(NOW);
  Element.prototype.scrollIntoView = () => {};
  clearJump();
});
afterEach(async () => {
  await h?.unmount();
  h = null;
  document.body.replaceChildren();
  vi.useRealTimers();
  clearJump();
});

const page = (hits: SearchHit[]): QueryResult => ({ search: { page: { hits, mode: "ranked" } } });
const hit = (over: Partial<SearchHit>): SearchHit => ({
  kind: "prompt",
  matter: MATTER,
  matter_title: THE_MATTER.name,
  thread: THREAD,
  thread_title: "Bail conditions",
  at: NOW,
  snippet: [{ text: "bail", hit: true }],
  ...over,
});
const isSearch = (q: HostQuery) => typeof q === "object" && "search" in q;

async function find(text: string) {
  await act(async () => {
    const input = document.querySelector<HTMLInputElement>('[data-testid="find"]');
    if (input === null) throw new Error("no Find");
    input.focus();
    Reflect.set(HTMLInputElement.prototype, "value", text, input);
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
  await act(async () => settled());
  await h?.settle();
}

async function press(testId: string) {
  await act(async () => h?.one(testId)?.click());
  await h?.settle();
  await h?.settle();
}

test("a jump to an entry the thread doesn't hold ends once paging reaches the thread's start", async () => {
  h = await mountRoom({
    query: (q) => {
      if (isSearch(q)) return page([hit({ order: 3 })]);
      // the thread's start: nothing older than its window
      if (typeof q === "object" && q.page_entries !== undefined) return { entries: [] };
      throw new Error("not scripted");
    },
  });
  // the window holds 5 to 8, and says there may be older: the entry asked for, 3, is gone
  const body = reset([
    userMessage(5, 5, "Q5"),
    agentText(6, 6, "A6"),
    userMessage(7, 7, "Q7"),
    agentText(8, 8, "A8"),
  ]);
  body.reset!.view_window.has_older = true;
  await h.deliverAwaited([body]);
  await h.settle();
  await find("bail");
  await press("find-conversation");
  const paged = h.transport
    .only("query")
    .filter((c) => typeof c.query === "object" && "page_entries" in c.query);
  expect(paged.length).toBeGreaterThan(0);
  expect(jumpAsked()).toBeNull();
});

test("a result found by its title clears a jump asked before", async () => {
  h = await mountRoom({
    query: (q) => {
      if (isSearch(q)) return page([hit({ kind: "thread_title" })]);
      throw new Error("not scripted");
    },
  });
  // a jump into a thread that never opened
  jumpTo(streamOf(T2), 3);
  await find("bail");
  await press("find-conversation");
  expect(jumpAsked()).toBeNull();
});

test("opening another conversation clears a jump to a thread that never opened", async () => {
  h = await mountRoom({
    threads: [summary(), { ...summary(T4), title: "Stay application" }],
  });
  jumpTo(streamOf(T2), 3);
  const other = h.all("conversation").find((c) => c.getAttribute("aria-current") !== "true");
  expect(other).toBeDefined();
  await h.click(other);
  await h.settle();
  expect(jumpAsked()).toBeNull();
});

test.each([
  // [the row's height, its clearance (the pin's band), the thread's visible height, where it lands]
  [120, 80, 400, "center"],
  [240, 80, 400, "center"],
  [241, 80, 400, "start"],
  [900, 80, 400, "start"],
  [120, 0, 100, "start"],
] as const)(
  "a found row %i px tall with %i px clearance in %i px lands at its %s, never under the pin (D-071)",
  (height, clearance, visible, block) => {
    expect(landing(height, clearance, visible)).toBe(block);
  },
);

test.each([
  // [the found words' top and bottom, the band's, the scroll that brings them into it]
  [[1400, 1424], [80, 600], 1072],
  [[200, 224], [80, 600], 0],
  [[80 + HIT_MARGIN, 120], [80, 600], 0],
  [[60, 84], [80, 600], -268],
  [[100, 900], [80, 600], 100 - 80 - HIT_MARGIN],
] as const)(
  "found words at %j with the band between the pin and the composer at %j scroll by %i (D-071)",
  ([top, bottom], [bandTop, bandBottom], dy) => {
    expect(nudge({ top, bottom }, { top: bandTop, bottom: bandBottom })).toBe(dy);
  },
);

// The thread's layout, faked: the scroller shows 600 px below the window's top, the pinned question's band is 80 px
// (the found row's scroll-margin), the composer stands under the scroller, the answer starts 100 px down the thread
// and runs 2000 px, its paragraph 200 px down, its table's body rows 24 px each from 184 px. scrollIntoView does nothing here, so a row lands where it was.
const ROWS = 40;
const BAND = { top: 80, bottom: 600 };
const TABLE = [
  "Long notes before the table.",
  "",
  "| Clause | Note |",
  "| --- | --- |",
  ...Array.from({ length: ROWS }, (_, i) => `| ${i + 1} | note ${i + 1} |`),
].join("\n");

function fakeLayout(): () => number {
  const box = () => document.querySelector<HTMLElement>(".room-thread");
  const top = () => box()?.scrollTop ?? 0;
  vi.spyOn(Element.prototype, "getBoundingClientRect").mockImplementation(function (this: Element) {
    const at = (y: number, h: number) => new DOMRect(0, y - top(), 600, h);
    if (this.classList.contains("room-thread")) return new DOMRect(0, 0, 600, 600);
    if (this.classList.contains("room-composer")) return new DOMRect(0, 600, 600, 140);
    if (this.matches("td")) {
      const tr = this.closest("tr");
      const i = tr === null ? 0 : [...tr.parentElement!.children].indexOf(tr);
      return at(184 + 24 * i, 24);
    }
    if (this.matches("p")) return at(200, 24);
    if (this.hasAttribute("data-entry-id")) return at(100, 2000);
    return new DOMRect();
  });
  const style = window.getComputedStyle.bind(window);
  vi.spyOn(window, "getComputedStyle").mockImplementation((el, pseudo) => {
    const real = style(el, pseudo);
    if (!el.hasAttribute("data-entry-id")) return real;
    return new Proxy(real, {
      get: (t, k) => (k === "scrollMarginTop" ? "80px" : Reflect.get(t, k)),
    });
  });
  const b = box();
  if (b === null) throw new Error("no thread");
  Object.defineProperty(b, "clientHeight", { configurable: true, value: 600 });
  Object.defineProperty(b, "scrollHeight", { configurable: true, value: 5000 });
  return top;
}

async function jumpWith(answer: string, snippet: SnippetPart[]): Promise<number> {
  h = await mountRoom({
    query: (q) => {
      if (isSearch(q)) return page([hit({ kind: "answer", order: 6, snippet })]);
      throw new Error("not scripted");
    },
  });
  await h.deliverAwaited([reset([userMessage(5, 5, "Q5"), agentText(6, 5, answer)])]);
  await h.settle();
  await find("note");
  const top = fakeLayout();
  await press("find-conversation");
  vi.restoreAllMocks();
  return top();
}

const said = (before: string, found: string, after: string): SnippetPart[] => [
  { text: before, hit: false },
  { text: found, hit: true },
  { text: after, hit: false },
];

test("a jump to words deep in a long answer's table lands on their cell, mid-band between pin and composer (D-071)", async () => {
  // "note 31" is in the 31st body row, 184 + 24 * 30 px down the thread: out of view below the composer
  const top = await jumpWith(TABLE, said("| 31 | ", "note", " 31 |"));
  const cell = 184 + 24 * 30 - top;
  expect(cell).toBeGreaterThanOrEqual(BAND.top + HIT_MARGIN);
  expect(cell + 24).toBeLessThanOrEqual(BAND.bottom - HIT_MARGIN);
  expect(Math.abs(cell + 12 - (BAND.top + BAND.bottom) / 2)).toBeLessThanOrEqual(1);
});

test("a jump to words at a long answer's start lands at its start, as before (D-071)", async () => {
  // "note 1" is in the first body row, already in view once the answer lands at its start
  expect(await jumpWith(TABLE, said("| 1 | ", "note", " 1 |\n| 2 |"))).toBe(0);
});

test("a jump to a short answer lands as before (D-071)", async () => {
  expect(await jumpWith("A short note on bail.", said("A short ", "note", " on bail."))).toBe(0);
});
