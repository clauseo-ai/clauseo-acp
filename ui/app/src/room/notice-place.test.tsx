// Where the room's own notices stand (notices.tsx's useNoticePlace): over the first run's welcome, which puts the wall
// away, the notice after Clauseo closed unexpectedly heads the work, not the hidden wall; in one column (under 1000 px)
// it heads the work too, before the thread, where the docked composer cannot cover it; from 1000 to 1199 px with a
// matter open it heads the wall; from 1200 px it stands at the rail's foot. happy-dom lays nothing out, so the window's
// width is the stubbed matchMedia's, and the place is read from the DOM.
// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, expect, test, vi } from "vitest";
import { FakeTransport } from "../transport/fake-transport.ts";
import type { LastExit, MatterSummary } from "../transport/protocol.ts";
import { fakeNext } from "./fake-next.ts";
import { A, CLAUDE, CODEX, settle } from "./first-run/rig.tsx";
import { Room } from "./room.tsx";
import { connectRoom } from "./start.ts";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

// the window's width, read by every media query at each look (rail-shown.ts makes its queries at import)
const view = vi.hoisted(() => {
  const state = { width: 1100 };
  const matches = (media: string) => {
    const min = /min-width:\s*([\d.]+)px/.exec(media);
    const max = /max-width:\s*([\d.]+)px/.exec(media);
    return (
      (min === null || state.width >= Number(min[1])) &&
      (max === null || state.width <= Number(max[1]))
    );
  };
  globalThis.matchMedia = (media: string) =>
    ({
      media,
      get matches() {
        return matches(media);
      },
      addEventListener: () => {},
      removeEventListener: () => {},
      addListener: () => {},
      removeListener: () => {},
      onchange: null,
      dispatchEvent: () => false,
    }) as unknown as MediaQueryList;
  return state;
});

let host: HTMLDivElement | null = null;
let root: Root | null = null;

afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  root = null;
  host = null;
  localStorage.clear();
});

const crashed: LastExit = { unclean: true, run_started: 1_790_000_000_000 };

/** The room over a fake host with the setup wire and the last_exit wire, listing `matters`, at `width` px. */
async function mount(width: number, matters: MatterSummary[]) {
  view.width = width;
  const transport = new FakeTransport({
    platform: "windows",
    query: async (query) => {
      if (query === "list_matters") return { matters };
      if (query === "last_exit") return { last_exit: crashed };
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
  (transport as unknown as { lastExitWire: boolean }).lastExitWire = true;
  connectRoom(transport, { language: "en" });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root?.render(<Room transport={transport} />));
  await settle();
  await settle();
}

// the first matter opened loads the thread's chunks: seconds on a loaded machine
const OPENS = 30_000;

const notice = () => document.querySelector<HTMLElement>(".crash-notice");

test("over the welcome, below 1200 px, the notice heads the work, not the wall the welcome puts away", async () => {
  await mount(1100, []);
  expect(document.querySelector('[data-testid="open-folder"]')).not.toBeNull();
  expect(notice()).not.toBeNull();
  expect(notice()?.closest(".room-notices")?.parentElement?.classList.contains("room-centre")).toBe(
    true,
  );
  expect(notice()?.closest(".room-pane")).toBeNull();
  expect(notice()?.querySelector('[data-testid="crash-got-it"]')).not.toBeNull();
});

test("Got it over the welcome leaves focus in the work: the welcome's first button, not the page", async () => {
  await mount(1100, []);
  const gotIt = notice()?.querySelector<HTMLButtonElement>('[data-testid="crash-got-it"]');
  expect(gotIt).not.toBeNull();
  await act(async () => gotIt?.click());
  expect(notice()).toBeNull();
  const focused = document.activeElement as HTMLElement | null;
  expect(focused).not.toBe(document.body);
  expect(focused?.closest(".room-centre")).not.toBeNull();
  expect(focused?.matches("button, a[href], input")).toBe(true);
});

test("over the welcome from 1200 px it stands at the rail's foot", async () => {
  await mount(1400, []);
  expect(notice()?.closest(".room-rail-foot")).not.toBeNull();
});

test(
  "from 1000 to 1199 px, a matter open, it heads the wall as before",
  async () => {
    await mount(1100, [A]);
    expect(notice()?.closest(".room-pane")).not.toBeNull();
    expect(document.querySelector(".room-notices")).toBeNull();
  },
  OPENS,
);

test(
  "in one column, a matter open, it heads the work before the thread, where the docked composer cannot cover it",
  async () => {
    await mount(800, [A]);
    expect(document.querySelector('[data-testid="open-folder"]')).toBeNull();
    const at = notice()?.closest(".room-notices");
    expect(at).not.toBeNull();
    expect(notice()?.closest(".room-pane")).toBeNull();
    // first in the work: the thread and the composer come after it
    expect(at?.parentElement?.firstElementChild).toBe(at);
  },
  OPENS,
);
