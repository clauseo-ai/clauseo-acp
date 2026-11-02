// @vitest-environment happy-dom
// Older pages against the live window: a Reset that starts the window later drops the pages and pages again, so no
// entry goes missing; a refused page waits until the view leaves the top and comes back.
import { act } from "react";
import { afterEach, expect, test, vi } from "vitest";
import { reset, STREAM, upsert, userMessage, agentText } from "../../testing/stream-script.ts";
import { entryIdOf, orderOf } from "../../store/view.ts";
import type { HostError, HostQuery, ViewEntry, ViewPatchBody } from "../../transport/protocol.ts";
import { mountRoom, type Harness } from "./harness.ts";

let h: Harness | null = null;
afterEach(async () => {
  await h?.unmount();
  h = null;
  document.body.replaceChildren();
});

/** Twelve entries, a question and its answer each turn, orders 1 to 12. */
const TWELVE: ViewEntry[] = Array.from({ length: 12 }, (_, i) =>
  i % 2 === 0 ? userMessage(i + 1, i + 1, `Q${i + 1}`) : agentText(i + 1, i + 1, `A${i + 1}`),
);
/** page_entries as the host answers it: the newest `limit` entries before `before_order`. */
const pageOf = (query: HostQuery) => {
  const ask = typeof query === "object" ? query.page_entries : undefined;
  if (ask === undefined) return { entries: [] };
  return {
    entries: TWELVE.filter((e) => orderOf(e) < (ask.before_order ?? Infinity)).slice(-ask.limit),
  };
};
const windowOf = (from: number, to: number) => {
  const body = reset(TWELVE.slice(from - 1, to));
  body.reset!.view_window.has_older = from > 1;
  return body;
};
const pageAsks = () =>
  h!.transport
    .only("query")
    .filter((c) => typeof c.query === "object" && "page_entries" in c.query);
const remove = (entry_id: string): ViewPatchBody => ({ remove: { entry_id } });
const shownIds = () =>
  [...h!.host.querySelectorAll<HTMLElement>("[data-entry-id]")].map((r) => r.dataset["entryId"]);

test("a Reset whose window starts later than the pages joined drops them and pages again: no entry goes missing", async () => {
  h = await mountRoom({ query: pageOf });
  // the window is 7 to 10; the page before it brings 1 to 6
  h.deliver([windowOf(7, 10)]);
  await h.settle();
  expect(shownIds()).toEqual(TWELVE.slice(0, 10).map(entryIdOf));
  // 11 and 12 stream in; then a Reset in the same epoch, as a subscriber that fell behind gets, starts at 9
  h.deliver([upsert(TWELVE[10]!), upsert(TWELVE[11]!)]);
  h.deliver([windowOf(9, 12)]);
  await h.settle();
  // 7 and 8 were in neither the old pages nor the new window: paged in again before 9
  expect(pageAsks().at(-1)?.query).toEqual({
    page_entries: { stream: STREAM, before_order: 9, limit: 50 },
  });
  expect(shownIds()).toEqual(TWELVE.map(entryIdOf));
});

test("a remove of the window's first entries keeps the pages: nothing is asked again and the history stays", async () => {
  h = await mountRoom({ query: pageOf });
  h.deliver([windowOf(7, 10)]);
  await h.settle();
  expect(pageAsks()).toHaveLength(1);
  // the first two live entries go, one patch at a time and then in one batch with an upsert after them
  h.deliver([remove(entryIdOf(TWELVE[6]!))]);
  await h.settle();
  h.deliver([remove(entryIdOf(TWELVE[7]!)), upsert(TWELVE[10]!)]);
  await h.settle();
  expect(pageAsks()).toHaveLength(1);
  expect(shownIds()).toEqual([...TWELVE.slice(0, 6), ...TWELVE.slice(8, 11)].map(entryIdOf));
  // a Reset that starts the window later after all that still drops them, and pages the gap back in
  h.deliver([windowOf(11, 12)]);
  await h.settle();
  expect(pageAsks().at(-1)?.query).toEqual({
    page_entries: { stream: STREAM, before_order: 11, limit: 50 },
  });
});

test("a refused page isn't asked again at the top; leaving the top and coming back asks once more", async () => {
  const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
  try {
    h = await mountRoom({
      query: () => {
        throw {
          code: "store.io",
          message: "for logs",
          retryable: true,
        } satisfies HostError;
      },
    });
    h.deliver([windowOf(7, 10)]);
    await h.settle();
    expect(pageAsks()).toHaveLength(1);
    const scrollTo = async (y: number) => {
      Object.defineProperty(window, "scrollY", { configurable: true, value: y });
      const el = h!.host.querySelector<HTMLElement>(".room-thread")!;
      Object.defineProperty(el, "scrollTop", { configurable: true, value: y });
      await act(async () => {
        window.dispatchEvent(new Event("scroll"));
        el.dispatchEvent(new Event("scroll"));
        // the scroll check runs once a frame
        await new Promise((resolve) => requestAnimationFrame(resolve));
      });
      await h!.settle();
    };
    await scrollTo(0);
    await scrollTo(0);
    expect(pageAsks()).toHaveLength(1);
    await scrollTo(400);
    expect(pageAsks()).toHaveLength(1);
    await scrollTo(0);
    expect(pageAsks()).toHaveLength(2);
  } finally {
    Reflect.deleteProperty(window, "scrollY");
    warn.mockRestore();
  }
});
