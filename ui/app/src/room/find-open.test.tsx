// @vitest-environment happy-dom
// Opening what Find found (U16a), in the real room over FakeTransport: a conversation found by its title opens; one
// found in something said opens at that entry, paged back to it and scrolled into view; a paper opens in its own app
// through its FileRefId. And Find's key, ⌘K on the Mac and Ctrl+K on Windows, puts the focus in Find from anywhere in
// the room.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { settled } from "@clauseo/components/lib/late.ts";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { entryIdOf, orderOf } from "../store/view.ts";
import { agentText, MATTER, reset, THREAD, userMessage, uuid } from "../testing/stream-script.ts";
import { FakeTransport, type FakeHost } from "../transport/fake-transport.ts";
import type {
  HostQuery,
  QueryResult,
  SearchHit,
  SearchRequest,
  ViewEntry,
} from "../transport/protocol.ts";
import { detailed, fakeNext } from "./fake-next.ts";
import { Room } from "./room.tsx";
import { connectRoom } from "./start.ts";
import { mountRoom, summary, THE_MATTER, type Harness } from "./thread/harness.ts";

const NOW = Date.UTC(2026, 10, 3, 17, 0);
const T2 = uuid(2, 1);

let h: Harness | null = null;
let root: Root | null = null;
let host: HTMLDivElement | null = null;
beforeEach(() => {
  vi.useFakeTimers({ toFake: ["Date"] });
  vi.setSystemTime(NOW);
});
afterEach(async () => {
  await h?.unmount();
  h = null;
  await act(async () => root?.unmount());
  root = null;
  host = null;
  document.body.replaceChildren();
  vi.useRealTimers();
  vi.restoreAllMocks();
});

const page = (hits: SearchHit[]): QueryResult => ({ search: { page: { hits, mode: "ranked" } } });
const hit = (over: Partial<SearchHit>, drop: readonly (keyof SearchHit)[] = []): SearchHit => {
  const made: SearchHit = {
    kind: "thread_title",
    matter: MATTER,
    matter_title: THE_MATTER.name,
    thread: T2,
    thread_title: "Surety for accused no. 2",
    at: NOW,
    snippet: [{ text: "Surety", hit: true }],
    ...over,
  };
  for (const key of drop) delete made[key];
  return made;
};
const searchOf = (q: HostQuery): SearchRequest | undefined =>
  typeof q === "object" && "search" in q ? q.search : undefined;

const field = () => document.querySelector<HTMLInputElement>('[data-testid="find"]');

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

/** Twelve entries, a question and its answer each turn, orders 1 to 12. */
const TWELVE: ViewEntry[] = Array.from({ length: 12 }, (_, i) =>
  i % 2 === 0 ? userMessage(i + 1, i + 1, `Q${i + 1}`) : agentText(i + 1, i + 1, `A${i + 1}`),
);

test("a conversation found by its title opens", async () => {
  h = await mountRoom({
    threads: [summary(), { ...summary(T2), title: "Surety for accused no. 2" }],
    query: (q) => {
      if (searchOf(q) !== undefined) return page([hit({})]);
      throw new Error("not scripted");
    },
  });
  await find("surety");
  const row = h.one("find-conversation");
  expect(row?.textContent).toContain("Surety for accused no. 2");
  await act(async () => row?.click());
  await h.settle();
  // Find is left, and the conversation is the open one on the rail
  expect(document.querySelector(".room-find-results")).toBeNull();
  const open = h.all("conversation").find((c) => c.getAttribute("aria-current") === "true");
  expect(open?.textContent).toContain("Surety for accused no. 2");
});

test("a conversation found in something said opens at that entry: paged back to it, then scrolled into view", async () => {
  const shown = vi.fn<(this: Element) => void>(function (this: Element) {});
  Element.prototype.scrollIntoView = shown;
  h = await mountRoom({
    query: (q) => {
      const s = searchOf(q);
      if (s !== undefined)
        return page([hit({ kind: "prompt", thread: THREAD, order: 3, thread_title: "Bail" })]);
      if (typeof q === "object" && q.page_entries !== undefined) {
        const ask = q.page_entries;
        return {
          entries: TWELVE.filter((e) => orderOf(e) < (ask.before_order ?? Infinity)).slice(
            -ask.limit,
          ),
        };
      }
      throw new Error("not scripted");
    },
  });
  // the open thread's window holds 9 to 12; 3 is three pages' worth above it at most
  const body = reset(TWELVE.slice(8));
  body.reset!.view_window.has_older = true;
  await h.deliverAwaited([body]);
  await h.settle();
  shown.mockClear();
  await find("Q3");
  const row = h.one("find-conversation");
  expect(row?.dataset["order"]).toBe("3");
  await act(async () => row?.click());
  await h.settle();
  await h.settle();
  const asked = h.transport
    .only("query")
    .filter((c) => typeof c.query === "object" && "page_entries" in c.query);
  expect(asked.length).toBeGreaterThan(0);
  const target = entryIdOf(TWELVE[2]!);
  expect(document.querySelector(`[data-entry-id="${target}"]`)).not.toBeNull();
  expect(shown.mock.contexts.map((el) => (el as HTMLElement).dataset["entryId"])).toContain(target);
});

test("a paper found by its file name opens its matter and the paper, by its FileRefId", async () => {
  const FILE = uuid(17, 8);
  h = await mountRoom({
    next: fakeNext({
      matters: () => [detailed(THE_MATTER)],
      threads: () => [{ ...summary(), seen: true }],
      papers: [
        { file: uuid(16, 8), path: "Bail/Order.pdf", size: 1, modified: 0 },
        { file: FILE, path: "Bail/Surety bond.pdf", size: 1, modified: 0 },
      ],
    }),
    query: (q) => {
      if (searchOf(q) !== undefined)
        return page([
          hit(
            {
              kind: "file_name",
              path: "Bail/Surety bond.pdf",
              snippet: [
                { text: "Surety", hit: true },
                { text: " bond.pdf", hit: false },
              ],
            },
            ["thread", "thread_title"],
          ),
        ]);
      throw new Error("not scripted");
    },
  });
  await find("surety");
  const row = h.one("find-paper");
  expect(row?.querySelector(".sr-only")?.textContent).toBe("Paper, Surety bond.pdf");
  expect(row?.querySelector(".room-find-hit")?.textContent).toBe("Surety");
  await act(async () => row?.click());
  await h.settle();
  expect(h.transport.only("open_file")).toEqual([{ call: "open_file", file: FILE }]);
});

/** The room alone, on `platform`, with one matter. */
async function mountOn(platform: NonNullable<FakeHost["platform"]>) {
  const transport = new FakeTransport({
    platform,
    next: fakeNext({ matters: () => [detailed(THE_MATTER)] }),
  });
  connectRoom(transport, { language: "en" });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root?.render(<Room transport={transport} />));
  for (let i = 0; i < 3; i += 1)
    await act(async () => new Promise((resolve) => setTimeout(resolve, 0)));
}

const press = (init: KeyboardEventInit) =>
  act(async () => {
    document.body.dispatchEvent(
      new KeyboardEvent("keydown", { bubbles: true, cancelable: true, ...init }),
    );
  });

test("⌘K focuses Find from anywhere in the room on the Mac; Ctrl+K there doesn't", async () => {
  await mountOn("mac");
  expect(field()?.getAttribute("aria-keyshortcuts")).toBe("Meta+K");
  expect(field()?.closest("label")?.querySelector("kbd")?.textContent).toBe("⌘K");
  expect(field()?.placeholder).toBe("Find");
  await press({ key: "k", code: "KeyK", ctrlKey: true });
  expect(document.activeElement).not.toBe(field());
  await press({ key: "k", code: "KeyK", metaKey: true });
  expect(document.activeElement).toBe(field());
});

test("Ctrl+K focuses Find on Windows", async () => {
  await mountOn("windows");
  expect(field()?.getAttribute("aria-keyshortcuts")).toBe("Control+K");
  expect(field()?.closest("label")?.querySelector("kbd")?.textContent).toBe("Ctrl+K");
  await press({ key: "k", code: "KeyK", metaKey: true });
  expect(document.activeElement).not.toBe(field());
  await press({ key: "k", code: "KeyK", ctrlKey: true });
  expect(document.activeElement).toBe(field());
});
