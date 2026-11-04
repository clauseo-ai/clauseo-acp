// @vitest-environment happy-dom
// axe-core over Find's results (U16a) in the real room, in both themes: matters and a paper found, conversations found by
// title and by what was said, one archived, the index still building; and nothing found. happy-dom computes no
// styles, so colour contrast is checked by hand (window.__clauseoAxe in the review), as for the room's own axe test.
import axe from "axe-core";
import { act } from "react";
import { settled } from "@clauseo/components/lib/late.ts";
import { afterEach, beforeAll, expect, test } from "vitest";
import { MATTER, THREAD, uuid } from "../testing/stream-script.ts";
import type { HostQuery, QueryResult, SearchHit } from "../transport/protocol.ts";
import { mountRoom, summary, THE_MATTER, type Harness } from "./thread/harness.ts";

const T2 = uuid(2, 1);
const T3 = uuid(3, 1);
const AT = Date.UTC(2026, 9, 27, 10, 0);

let h: Harness | null = null;
beforeAll(() => {
  document.title = "Clauseo";
});
afterEach(async () => {
  await h?.unmount();
  h = null;
  document.body.replaceChildren();
});

const HITS: SearchHit[] = [
  { kind: "matter_title", matter: MATTER, matter_title: THE_MATTER.name, at: AT, snippet: [] },
  {
    kind: "file_name",
    matter: MATTER,
    path: "Bail/Surety bond.pdf",
    at: AT,
    snippet: [
      { text: "Surety", hit: true },
      { text: " bond.pdf", hit: false },
    ],
  },
  {
    kind: "thread_title",
    matter: MATTER,
    thread: T3,
    thread_title: "Surety, the old order",
    at: AT,
    snippet: [{ text: "Surety", hit: true }],
  },
  {
    kind: "answer",
    matter: MATTER,
    thread: THREAD,
    order: 4,
    at: AT,
    snippet: [
      { text: "A personal bond with one ", hit: false },
      { text: "surety", hit: true },
      { text: ".", hit: false },
    ],
  },
];

const answer =
  (hits: SearchHit[]) =>
  (q: HostQuery): QueryResult => {
    if (typeof q === "object" && "search" in q)
      return {
        search: {
          page: {
            hits,
            mode: "ranked",
            ...(hits.length > 0 ? { indexing: { done: 3, total: 9 } } : {}),
          },
        },
      };
    throw new Error("not scripted");
  };

async function findIn(theme: "light" | "dark", hits: SearchHit[], words: string) {
  h = await mountRoom({
    threads: [summary(), summary(T2), summary(T3, "archived")],
    query: answer(hits),
    display: { theme, language: "en" },
  });
  await act(async () => {
    const input = document.querySelector<HTMLInputElement>('[data-testid="find"]');
    input?.focus();
    if (input === null) return;
    Reflect.set(HTMLInputElement.prototype, "value", words, input);
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
  await act(async () => settled());
  await h.settle();
}

const violations = async () =>
  (await axe.run(document, { resultTypes: ["violations"] })).violations.map(
    (v) => `${v.id} at ${v.nodes.map((n) => n.target.join(" ")).join(", ")}`,
  );

for (const theme of ["light", "dark"] as const) {
  test(`axe finds no violation in Find's results: ${theme}`, async () => {
    await findIn(theme, HITS, "surety");
    const results = document.querySelector(".room-find-results");
    expect(results?.querySelector(".room-find-indexing")).not.toBeNull();
    expect(results?.querySelectorAll('[data-testid="find-conversation"]')).toHaveLength(2);
    expect(results?.querySelector(".room-find-archived")).not.toBeNull();
    expect(results?.querySelector('[data-testid="find-paper"]')).not.toBeNull();
    expect(document.documentElement.dataset["theme"]).toBe(theme === "dark" ? "night" : "day");
    expect(await violations()).toEqual([]);
  }, 20_000);

  test(`axe finds no violation when nothing is found: ${theme}`, async () => {
    await findIn(theme, [], "zzz");
    expect(document.querySelector(".room-find-results")?.textContent).toContain(
      'Nothing found for "zzz".',
    );
    expect(await violations()).toEqual([]);
  }, 20_000);
}
