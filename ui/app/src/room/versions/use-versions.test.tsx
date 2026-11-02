// @vitest-environment happy-dom
// Versions belongs to the matter it opened on: opening another matter closes it, coming back finds it closed, and a
// chunk that loads after the lawyer moved on opens nothing. A stylesheet that failed to load is asked for again.
import { act, createElement, useEffect } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, expect, test, vi } from "vitest";
import { createViewStore } from "../../store/store.ts";
import { manualBatch } from "../../testing/dom.ts";
import { FakeTransport } from "../../transport/fake-transport.ts";
import type { MatterId } from "../../transport/protocol.ts";
import { loadSheet } from "./versions-chunk.ts";
import { loadVersions, useVersions, type VersionsSlots } from "./use-versions.ts";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const A = "01900002-0000-7000-8000-00000000000a";
const B = "01900002-0000-7000-8000-00000000000b";

let root: Root | null = null;
afterEach(async () => {
  await act(async () => root?.unmount());
  root = null;
});

/** useVersions in a bare component over a FakeTransport; `slots` reads what it last gave. */
async function mount() {
  const transport = new FakeTransport({
    query: async () => ({ threads: [] }),
  });
  const store = createViewStore(transport, { batch: () => manualBatch().batch });
  let last: VersionsSlots | null = null;
  const seen = (slots: VersionsSlots) => {
    last = slots;
  };
  const Probe = ({ matter }: { matter: MatterId | null }) => {
    const slots = useVersions({ transport, store, matter, papers: null, platform: "mac" });
    useEffect(() => seen(slots));
    return null;
  };
  const host = document.createElement("div");
  const r = createRoot(host);
  root = r;
  const show = (matter: MatterId | null) =>
    act(async () => r.render(createElement(Probe, { matter })));
  await show(A);
  return {
    show,
    slots: (): VersionsSlots => {
      if (last === null) throw new Error("not rendered");
      return last;
    },
  };
}

const loaded = () => act(async () => void (await loadVersions()));

test("opening another matter closes Versions, and coming back finds it closed", async () => {
  const m = await mount();
  await act(async () => m.slots().toolbar.onVersions());
  await loaded();
  expect(m.slots().open).toBe(true);
  await m.show(B);
  expect(m.slots().open).toBe(false);
  await m.show(A);
  expect(m.slots().open).toBe(false);
});

test("Versions' chunk landing after the lawyer opened another matter opens nothing", async () => {
  const m = await mount();
  await act(async () => {
    m.slots().toolbar.onVersions();
    await m.show(B);
  });
  await loaded();
  expect(m.slots().open).toBe(false);
  await m.show(A);
  expect(m.slots().open).toBe(false);
});

test("a stylesheet that failed to load is asked for again on the next open", async () => {
  // the links the sheet adds, held here: the test environment loads no stylesheet of its own
  const links: Node[] = [];
  const append = vi.spyOn(document.head, "append").mockImplementation((...nodes) => {
    for (const n of nodes) if (typeof n !== "string") links.push(n);
  });
  try {
    const first = loadSheet(true);
    expect(links).toHaveLength(1);
    links[0]?.dispatchEvent(new Event("error"));
    await first;
    const second = loadSheet(true);
    expect(links).toHaveLength(2);
    links[1]?.dispatchEvent(new Event("load"));
    await second;
    // loaded, it is kept: a third open asks for nothing
    expect(loadSheet(true)).toBe(second);
    expect(links).toHaveLength(2);
  } finally {
    append.mockRestore();
  }
});
