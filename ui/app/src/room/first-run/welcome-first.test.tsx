// The welcome's first paint (welcome-fast): the first run is in the first load, so once `list_matters` answers the
// welcome's primary button is in the next commit, with no Suspense fallback on the way (a lazy first run's fallback
// held the welcome back for React's 300 ms reveal throttle); the thread's and Snooze's chunks stay unloaded under the
// welcome; and the start-up marks go once, after the button, through the transport's pageMarks.
// @vitest-environment happy-dom
import { act, Suspense } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { FakeTransport } from "../../transport/fake-transport.ts";
import type { PageMarks } from "../../transport/bindings.ts";
import { MARKS, resetMarks } from "../../start-marks.ts";
import { fakeNext } from "../fake-next.ts";
import { Room } from "../room.tsx";
import { connectRoom } from "../start.ts";
import { A, CLAUDE, CODEX, settle } from "./rig.tsx";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

// each chunk's module evaluations: a module loads once per file, so the tests that expect none come before the one
// that opens a matter
const loaded = vi.hoisted(() => ({ list: 0, snooze: 0 }));
vi.mock("../thread/list.tsx", async (actual) => {
  loaded.list += 1;
  return actual();
});
vi.mock("../snooze-late.tsx", async (actual) => {
  loaded.snooze += 1;
  return actual();
});

let host: HTMLDivElement | null = null;
let root: Root | null = null;

beforeEach(() => {
  resetMarks();
  loaded.list = 0;
  loaded.snooze = 0;
});

afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  root = null;
  host = null;
});

/** The room over a fake host with the setup wire, listing `matters`, inside a boundary whose fallback counts each
 * time it shows. */
async function mount(matters = [] as (typeof A)[]) {
  const transport = new FakeTransport({
    platform: "windows",
    query: async (query) => {
      if (query === "list_matters") return { matters };
      if (typeof query === "object" && "list_threads" in query) return { threads: [] };
      throw new Error(`not scripted: ${JSON.stringify(query)}`);
    },
    setup: { first: () => [] },
    next: fakeNext({
      matters: () => [],
      agents: [CLAUDE, CODEX].map((id) => ({ id })),
      parts: ["agents"],
    }),
  });
  const sent: PageMarks[] = [];
  const order: string[] = [];
  transport.pageMarks = async (read) => {
    order.push(document.querySelector('[data-testid="open-folder"]') === null ? "before" : "after");
    sent.push(read());
  };
  connectRoom(transport, { language: "en" });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  const fallbacks = { shown: 0 };
  function Fallback() {
    fallbacks.shown += 1;
    return null;
  }
  await act(async () =>
    root?.render(
      <Suspense fallback={<Fallback />}>
        <Room transport={transport} />
      </Suspense>,
    ),
  );
  return { transport, fallbacks, sent, order };
}

const openFolder = () => document.querySelector('[data-testid="open-folder"]');

test("with list_matters answered, the welcome's button is in the same act as the first render: no fallback, no chunk", async () => {
  const { fallbacks, transport } = await mount();
  expect(transport.calls.some((c) => c.call === "query" && c.query === "list_matters")).toBe(true);
  expect(openFolder()).not.toBeNull();
  expect(openFolder()?.hasAttribute("disabled")).toBe(false);
  expect(fallbacks.shown).toBe(0);
  await settle();
  expect(fallbacks.shown).toBe(0);
});

test("under the welcome neither the thread's chunk nor Snooze's loads", async () => {
  await mount();
  await settle();
  expect(openFolder()).not.toBeNull();
  expect(loaded).toEqual({ list: 0, snooze: 0 });
});

test("a matter open loads the thread's chunk, as before", async () => {
  await mount([A]);
  await settle();
  expect(openFolder()).toBeNull();
  expect(loaded.list).toBeGreaterThan(0);
});

test("the marks go once, after the button mounts, each taken", async () => {
  const { sent, order } = await mount();
  await settle();
  expect(sent).toHaveLength(1);
  expect(order).toEqual(["after"]);
  const [marks] = sent;
  for (const age of [marks?.welcome, marks?.button, marks?.script])
    expect(age).toBeTypeOf("number");
  // the welcome and its button came after the script started, and the page's origin before all of them
  expect(marks!.button!).toBeLessThanOrEqual(marks!.script!);
  expect(marks!.script!).toBeLessThanOrEqual(marks!.navigation);
  expect(performance.getEntriesByName(MARKS.button, "mark")).toHaveLength(1);
});

test("a start with a matter already there sends no marks", async () => {
  const { sent } = await mount([A]);
  await settle();
  expect(sent).toEqual([]);
});
