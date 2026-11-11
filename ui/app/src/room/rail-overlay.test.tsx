// @vitest-environment happy-dom
// D-061: under 1000 px Matters opens the list as a panel over the work (responsive.css; its rules are
// components/clauseo/rail-overlay.test.ts's). Here, in a window 760 px wide: Matters opens and closes it, Escape in it
// folds it and gives focus back to Matters, and opening it changes nothing in the work: the same composer, the same
// thread, no attribute on the window or the work, so nothing under the panel moves.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, test } from "vitest";
import type { MatterSummary } from "../transport/protocol.ts";
import { FakeTransport } from "../transport/fake-transport.ts";
import { uuid } from "../testing/stream-script.ts";
import { frameBatch } from "../store/batch.ts";
import { StoreContext } from "../store/hooks.ts";
import { createViewStore } from "../store/store.ts";
import { Room } from "./room.tsx";
import { stubFonts } from "./thread/testing.ts";
import { connectRoom } from "./start.ts";
import { loadThread } from "./thread/chunk.ts";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
stubFonts();

const A: MatterSummary = { matter: uuid(1, 2), name: "Acme Ltd v. State", kind: "folder" };
const B: MatterSummary = { matter: uuid(2, 2), name: "Mehta v. Mehta", kind: "folder" };

let host: HTMLDivElement | null = null;
let root: Root | null = null;
let width = 0;

beforeEach(() => {
  width = window.innerWidth;
  // under 1000 px: the one-column room
  Object.defineProperty(window, "innerWidth", { configurable: true, value: 760 });
});

afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  root = null;
  host = null;
  Object.defineProperty(window, "innerWidth", { configurable: true, value: width });
});

async function mount() {
  const transport = new FakeTransport({
    query: (query) => {
      if (query === "list_matters") return { matters: [A, B] };
      if (typeof query === "object" && query.list_threads !== undefined) return { threads: [] };
      throw new Error("not scripted");
    },
  });
  connectRoom(transport, {});
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () =>
    root?.render(
      <StoreContext value={createViewStore(transport, { batch: () => frameBatch })}>
        <Room transport={transport} />
      </StoreContext>,
    ),
  );
  await act(async () => new Promise((resolve) => setTimeout(resolve, 0)));
  await act(async () => void (await loadThread()));
}

const toggle = () => host?.querySelector<HTMLButtonElement>("[data-testid=rail-toggle]") ?? null;
const rail = () => host?.querySelector<HTMLElement>("nav.room-rail") ?? null;

test("under 1000 px Matters opens the list over the work and closes it again, and the work stays as it was", async () => {
  await mount();
  const composer = host?.querySelector(".room-composer");
  const thread = host?.querySelector(".room-thread");
  expect(composer).not.toBeNull();
  expect(rail()?.hasAttribute("data-open")).toBe(false);
  // what opening changes on the window and the work, and where they stand in the page
  const changed: string[] = [];
  const watch = new MutationObserver((records) => {
    for (const r of records) changed.push(`${(r.target as Element).className} ${r.attributeName}`);
  });
  for (const part of [host?.querySelector(".room-window"), host?.querySelector(".room-centre")])
    if (part != null) watch.observe(part, { attributes: true });
  await act(async () => toggle()?.click());
  expect(toggle()?.getAttribute("aria-expanded")).toBe("true");
  expect(rail()?.hasAttribute("data-open")).toBe(true);
  // the same composer and thread, unmoved in the page: the panel pushes nothing below it
  expect(host?.querySelector(".room-composer")).toBe(composer);
  expect(host?.querySelector(".room-thread")).toBe(thread);
  watch.disconnect();
  expect(changed.filter((c) => !c.endsWith(" style"))).toEqual([]);
  // Matters again closes it
  await act(async () => toggle()?.click());
  expect(rail()?.hasAttribute("data-open")).toBe(false);
  expect(toggle()?.getAttribute("aria-expanded")).toBe("false");
});

test("under 1000 px Escape in the open list folds it, and focus goes back to Matters", async () => {
  await mount();
  await act(async () => toggle()?.click());
  const row = rail()?.querySelector<HTMLButtonElement>("[data-testid=matter-row]");
  expect(row).not.toBeNull();
  await act(async () => row?.focus());
  await act(async () =>
    document.activeElement?.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Escape", bubbles: true }),
    ),
  );
  expect(rail()?.hasAttribute("data-open")).toBe(false);
  expect(document.activeElement).toBe(toggle());
});
