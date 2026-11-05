// @vitest-environment happy-dom
// A Find result's jump to an entry (thread/jump-entry.ts) never outlives its open (U16a): paging back to the thread's
// start without the entry ends it, a result found by its title clears it, and so does opening another conversation,
// so a thread opened days later doesn't page back to an old entry.
import { act } from "react";
import { settled } from "@clauseo/components/lib/late.ts";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { streamOf } from "../store/view.ts";
import { agentText, MATTER, reset, THREAD, userMessage, uuid } from "../testing/stream-script.ts";
import type { HostQuery, QueryResult, SearchHit } from "../transport/protocol.ts";
import { clearJump, jumpAsked, jumpTo } from "./thread/jump-entry.ts";
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
