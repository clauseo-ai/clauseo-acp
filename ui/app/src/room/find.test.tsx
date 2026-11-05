// @vitest-environment happy-dom
// Find searching inside (U16a) in the real room over FakeTransport: a search on every change of the field, its
// generations counting up; an older answer, or "cancelled", dropped; the found words marked as spans, a snippet's
// markup staying text; RO-27 while the index builds; the room's own words for a missing title; archived results
// marked; nothing found; and zero timers at rest once a search settles. Opening a result, Find's key, the lazy chunk
// and axe are in find-open.test.tsx, find-lazy.test.tsx and find-axe.test.tsx.
import { act, StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { iconIn } from "@clauseo/components/icons/icon-in.tsx";
import { IconChecking } from "@clauseo/components/icons/icons.tsx";
import { settled } from "@clauseo/components/lib/late.ts";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { Search } from "../store/search.ts";
import { MATTER, THREAD, uuid } from "../testing/stream-script.ts";
import { FakeTransport } from "../transport/fake-transport.ts";
import type {
  HostQuery,
  QueryResult,
  SearchHit,
  SearchPage,
  SearchRequest,
} from "../transport/protocol.ts";
import { detailed, fakeNext } from "./fake-next.ts";
import { Room } from "./room.tsx";
import { connectRoom } from "./start.ts";
import { mountRoom, summary, THE_MATTER, type Harness } from "./thread/harness.ts";

const NOW = Date.UTC(2026, 10, 3, 17, 0);
const T2 = uuid(2, 1);
const T3 = uuid(3, 1);

let h: Harness | null = null;
beforeEach(() => {
  vi.useFakeTimers({ toFake: ["Date"] });
  vi.setSystemTime(NOW);
});
afterEach(async () => {
  await h?.unmount();
  h = null;
  document.body.replaceChildren();
  vi.useRealTimers();
});

const page = (hits: SearchHit[], over: Partial<SearchPage> = {}): QueryResult => ({
  search: { page: { hits, mode: "ranked", ...over } },
});

/** A hit in the open matter, said by the lawyer at NOW. */
const said = (
  over: Partial<SearchHit> = {},
  drop: readonly (keyof SearchHit)[] = [],
): SearchHit => {
  const made: SearchHit = {
    kind: "prompt",
    matter: MATTER,
    matter_title: "Sharma v. State of Delhi",
    thread: THREAD,
    thread_title: "Bail conditions",
    order: 3,
    at: NOW,
    snippet: [
      { text: "Can we offer a ", hit: false },
      { text: "surety", hit: true },
      { text: " instead?", hit: false },
    ],
    ...over,
  };
  for (const key of drop) delete made[key];
  return made;
};

const searchOf = (q: HostQuery): SearchRequest | undefined =>
  typeof q === "object" && "search" in q ? q.search : undefined;
const searches = (t: FakeTransport) => t.only("query").flatMap((c) => searchOf(c.query) ?? []);

const field = () => document.querySelector<HTMLInputElement>('[data-testid="find"]');
const results = () => document.querySelector<HTMLElement>(".room-find-results");

/** Types `text` into Find as the lawyer would, then lets the search and the results' chunk land. */
async function find(text: string) {
  await act(async () => {
    const input = field();
    if (input === null) throw new Error("no Find");
    input.focus();
    Reflect.set(HTMLInputElement.prototype, "value", text, input);
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
  await act(async () => settled());
  await h?.settle();
}

/** A room whose host answers each search with `answer`. */
async function room(answer: (s: SearchRequest) => QueryResult, threads = [summary()]) {
  h = await mountRoom({
    threads,
    query: (q) => {
      const s = searchOf(q);
      if (s !== undefined) return answer(s);
      throw new Error(`not scripted: ${JSON.stringify(q)}`);
    },
  });
  return h;
}

test("a new search on every change of the field: one per keystroke, generations counting up, none for empty words", async () => {
  const { transport } = await room(() => page([]));
  for (const text of ["a", "as", "ask"]) await find(text);
  const sent = searches(transport);
  expect(sent.map((s) => s.query)).toEqual(["a", "as", "ask"]);
  expect(sent.map((s) => s.generation)).toEqual(
    sent.map((s) => s.generation).sort((a, b) => a - b),
  );
  expect(new Set(sent.map((s) => s.generation)).size).toBe(3);
  for (const s of sent) {
    expect(s.scope).toBe("all");
    expect(s.include_archived).toBe(true);
    expect(s.limit).toBe(20);
    expect(s.cursor).toBeUndefined();
  }
  // emptied: nothing is sent, and the date groups come back
  await find("");
  expect(searches(transport)).toHaveLength(3);
});

/** A transport whose searches wait until each is answered by hand. */
function held() {
  const waiting: { request: SearchRequest; answer: (r: QueryResult) => void }[] = [];
  const transport = new FakeTransport({
    query: (q) =>
      new Promise<QueryResult>((answer) => {
        const request = searchOf(q);
        if (request !== undefined) waiting.push({ request, answer });
      }),
  });
  return { transport, waiting };
}

test("an older answer arriving after a newer one is dropped", async () => {
  const { transport, waiting } = held();
  const search = new Search(transport);
  search.start();
  search.set("ask");
  search.set("asks");
  expect(waiting.map((w) => w.request.query)).toEqual(["ask", "asks"]);
  waiting[1]?.answer(page([said({ thread_title: "Newer" })]));
  await Promise.resolve();
  await Promise.resolve();
  expect(search.state().hits.map((x) => x.thread_title)).toEqual(["Newer"]);
  waiting[0]?.answer(page([said({ thread_title: "Older" })]));
  await Promise.resolve();
  await Promise.resolve();
  expect(search.state().hits.map((x) => x.thread_title)).toEqual(["Newer"]);
  expect(search.state().answered).toBe("asks");
});

test('"cancelled" is dropped: the hits stay as they were', async () => {
  const { transport, waiting } = held();
  const search = new Search(transport);
  search.start();
  search.set("bail");
  waiting[0]?.answer(page([said()]));
  await Promise.resolve();
  await Promise.resolve();
  search.set("bail ");
  waiting[1]?.answer({ search: "cancelled" });
  await Promise.resolve();
  await Promise.resolve();
  expect(search.state().hits).toHaveLength(1);
  expect(search.state().answered).toBe("bail");
  expect(search.state().settled).toBe(false);
});

test("a refused search shows nothing new, and never its message", async () => {
  const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
  try {
    const transport = new FakeTransport({
      query: () => {
        throw { code: "search.unavailable", message: "a detail for logs", retryable: true };
      },
    });
    const search = new Search(transport);
    search.start();
    search.set("bail");
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(search.state()).toMatchObject({ hits: [], answered: "", settled: true });
    expect(warn.mock.calls.flat().join(" ")).not.toContain("a detail for logs");
  } finally {
    warn.mockRestore();
  }
});

test("the found words are spans in primary at chip weight; markup inside a snippet stays text", async () => {
  await room(() =>
    page([
      said({
        snippet: [
          { text: "the <b>old</b> bond and a ", hit: false },
          { text: "surety", hit: true },
        ],
      }),
    ]),
  );
  await find("surety");
  const row = results()?.querySelector('[data-testid="find-conversation"]');
  expect(row?.textContent).toContain("You: the <b>old</b> bond and a surety");
  expect(row?.querySelector("b")).toBeNull();
  expect([...(row?.querySelectorAll(".room-find-hit") ?? [])].map((s) => s.textContent)).toEqual([
    "surety",
  ]);
  expect(results()?.textContent).toContain("Conversations");
  expect(row?.textContent).toContain("Sharma v. State of Delhi, today");
});

test("RO-27 shows while the index builds, and clears on a page without it", async () => {
  await room((s) => page([said()], s.query === "ask" ? { indexing: { done: 3, total: 10 } } : {}));
  await find("ask");
  const line = results()?.querySelector<HTMLElement>(".room-find-indexing");
  expect(line?.textContent).toBe("Still adding older conversations.");
  // no clock, which means Answering on the rail; what the line means, RO-32, behind the info icon's app tooltip
  expect(iconIn(line, IconChecking, 14, "room-matter-mark")).toBeNull();
  expect(line?.querySelectorAll("svg").length).toBe(1);
  const info = line?.querySelector<HTMLButtonElement>("button");
  expect(info?.getAttribute("aria-label")).toBe("Some may not show yet.");
  await act(async () => {
    info?.focus();
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
  expect(document.querySelector("[data-base-ui-portal] > [data-side]")?.textContent).toBe(
    "Some may not show yet.",
  );
  await find("asks");
  expect(results()?.querySelector(".room-find-indexing")).toBeNull();
});

test("missing titles show the room's own words; an unknown matter's line keeps its day; an archived result says so", async () => {
  const OTHER = uuid(7, 2);
  await room(
    () =>
      page([
        said({ kind: "thread_title", thread: T2 }, ["thread_title", "matter_title"]),
        said({ kind: "answer", matter: OTHER }, ["matter_title", "thread_title"]),
        said({ kind: "thread_title", thread: T3, thread_title: "Old bail order" }),
        said({ kind: "unknown" }),
      ]),
    // T3 archived: the host's list leaves it out
    [summary(), summary(T2)],
  );
  await find("bail");
  const rows = [...(results()?.querySelectorAll('[data-testid="find-conversation"]') ?? [])];
  expect(rows.map((r) => r.querySelector(".room-matter-name")?.textContent)).toEqual([
    "New conversation",
    "Old bail orderArchived",
    "New conversation",
  ]);
  expect(rows.map((r) => r.querySelector(".room-matter-note")?.textContent)).toEqual([
    "Sharma v. State of Delhi, today",
    "Sharma v. State of Delhi, today",
    "today",
  ]);
  expect(rows[2]?.textContent).toContain("Clauseo:");
  // archived: the icon, named in words and in the app's tooltip, the word itself not shown
  const mark = rows[1]?.querySelector(".room-find-archived");
  expect(mark?.hasAttribute("title")).toBe(false);
  expect(mark?.querySelector("svg")?.getAttribute("aria-hidden")).toBe("true");
  expect(mark?.querySelector(".sr-only")?.textContent).toBe("Archived");
  expect(rows[0]?.querySelector(".room-find-archived")).toBeNull();
  // the tooltip opens for the keyboard: the row's focus shows it, in the floating layer, outside the rail
  await act(async () => {
    (rows[1] as HTMLElement | undefined)?.focus();
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
  const tip = document.querySelector<HTMLElement>("[data-base-ui-portal] > [data-side]");
  expect(tip?.textContent).toBe("Archived");
  expect(tip?.closest(".room-rail")).toBeNull();
  expect(tip?.classList.contains("z-30")).toBe(true);
});

test("a conversation's day reads as Recent's: today, else its weekday and date, never one alone", async () => {
  const DAY = 86_400_000;
  await room(() =>
    page([
      said({ kind: "thread_title", thread: T2, at: NOW - DAY }),
      said({ kind: "thread_title", thread: T3, at: NOW - 8 * DAY }),
      said({ at: NOW }),
    ]),
  );
  await find("bail");
  const notes = [...(results()?.querySelectorAll('[data-testid="find-conversation"]') ?? [])].map(
    (r) => r.querySelector(".room-matter-note")?.textContent,
  );
  expect(notes).toEqual([
    "Sharma v. State of Delhi, Monday 2 November",
    "Sharma v. State of Delhi, Monday 26 October",
    "Sharma v. State of Delhi, today",
  ]);
});

test("nothing found says so, with the scope behind the info icon", async () => {
  await room(() => page([]));
  await find("zzz");
  expect(results()?.textContent).toContain('Nothing found for "zzz".');
  const scope = results()?.querySelector(".room-find-scope");
  expect(scope?.getAttribute("aria-label")).toBe("Searches titles, paper names and conversations.");
});

test("zero timers at rest once a search settles", async () => {
  await room(() => page([said()], { indexing: { done: 1, total: 2 } }));
  // the results' chunk in first, with real timers
  await find("s");
  vi.useFakeTimers();
  await act(async () => {
    const input = field();
    if (input === null) return;
    Reflect.set(HTMLInputElement.prototype, "value", "surety", input);
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
  await act(async () => {
    await vi.runAllTimersAsync();
    await vi.dynamicImportSettled();
  });
  expect(results()?.textContent).toContain("Bail conditions");
  expect(vi.getTimerCount()).toBe(0);
  await act(() => vi.advanceTimersByTimeAsync(5000));
  expect(vi.getTimerCount()).toBe(0);
});

test("under StrictMode (main.tsx's), the results mounting with the first word typed still show its answer", async () => {
  const transport = new FakeTransport({
    next: fakeNext({ matters: () => [detailed(THE_MATTER)], threads: () => [summary()] }),
    query: (q) => {
      if (searchOf(q) !== undefined) return page([said()]);
      throw new Error("not scripted");
    },
  });
  connectRoom(transport, { language: "en" });
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () =>
      root.render(
        <StrictMode>
          <Room transport={transport} />
        </StrictMode>,
      ),
    );
    await act(async () => new Promise((resolve) => setTimeout(resolve, 0)));
    await find("surety");
    await act(async () => new Promise((resolve) => setTimeout(resolve, 0)));
    expect(results()?.textContent).toContain("Bail conditions");
  } finally {
    await act(async () => root.unmount());
  }
});
