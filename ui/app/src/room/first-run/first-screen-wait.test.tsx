// The first screen waits for Matters' first list (deciding.ts): on a slow start the list lands after the room's first
// render, and the room's composer layout showed for 250–500 ms before the welcome (the Windows VM, after a reboot and on
// a fresh profile). While the list is out, the room is laid out unseen and the page's mark stands; then the welcome, or
// the room. A failed list shows the room at once, and a list that never comes shows it after DECIDE_MS, as before.
// @vitest-environment happy-dom
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, expect, test, vi } from "vitest";
import { FakeTransport } from "../../transport/fake-transport.ts";
import { fakeUpdate } from "../../transport/fake-update.ts";
import type { MatterSummary } from "../../transport/protocol.ts";
import { DECIDE_MS } from "../deciding.ts";
import { fakeNext } from "../fake-next.ts";
import { Room } from "../room.tsx";
import { connectRoom } from "../start.ts";
import { A, CLAUDE, CODEX, settle } from "./rig.tsx";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement | null = null;
let root: Root | null = null;

afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  root = null;
  host = null;
  vi.useRealTimers();
});

/** The room over a fake host with the setup wire, whose `list_matters` answers only when the test says. */
async function mount({ update = false } = {}) {
  let answer: (matters: MatterSummary[]) => void = () => {};
  let refuse: (error: unknown) => void = () => {};
  const listed = new Promise<{ matters: MatterSummary[] }>((resolve, reject) => {
    answer = (matters) => resolve({ matters });
    refuse = reject;
  });
  const transport = new FakeTransport({
    platform: "windows",
    query: async (query) => {
      if (query === "list_matters") return listed;
      if (typeof query === "object" && "list_threads" in query) return { threads: [] };
      throw new Error(`not scripted: ${JSON.stringify(query)}`);
    },
    setup: { first: () => [] },
    ...(update
      ? {
          update: fakeUpdate({
            view: { current: "0.0.1", state: { ready: { version: "0.0.2" } }, mode: "auto" },
          }),
        }
      : {}),
    next: fakeNext({
      matters: () => [],
      agents: [CLAUDE, CODEX].map((id) => ({ id })),
      parts: ["agents"],
    }),
  });
  connectRoom(transport, { language: "en" });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root?.render(<Room transport={transport} />));
  return {
    answer: async (matters: MatterSummary[]) => {
      await act(async () => answer(matters));
      await settle();
    },
    refuse: async () => {
      await act(async () => refuse({ code: "internal" }));
      await settle();
    },
  };
}

const mark = () => document.querySelector("#first-mark");
const deciding = () => document.querySelector(".room-window[data-deciding]");
/** `selector` as the lawyer would see it: one outside a room still deciding (which shell.css keeps unseen). */
const seen = (selector: string) =>
  [...document.querySelectorAll(selector)].find((c) => c.closest("[data-deciding]") === null) ??
  null;
const seenComposer = () => seen(".room-composer");
const welcome = () => document.querySelector('[data-testid="open-folder"]');

test("shell.css keeps a deciding room unseen", () => {
  const sheet = readFileSync(
    join(dirname(fileURLToPath(import.meta.url)), "../../../../components/src/clauseo/shell.css"),
    "utf8",
  );
  expect(sheet).toMatch(/\.room-window\[data-deciding\]\s*\{\s*visibility: hidden;/);
  // the mark rides inside the unseen window: index.html's own style keeps it seen
  const page = readFileSync(
    join(dirname(fileURLToPath(import.meta.url)), "../../../index.html"),
    "utf8",
  );
  expect(page).toMatch(/#first-mark \{[^}]*position: fixed;[^}]*visibility: visible;/);
});

test("while deciding, a ready update's notice stays unseen; it shows with the room", async () => {
  const { answer } = await mount({ update: true });
  await settle();
  expect(mark()).not.toBeNull();
  // laid out, in the unseen window
  expect(document.querySelector(".update-notice")?.closest("[data-deciding]")).not.toBeNull();
  expect(seen(".update-notice")).toBeNull();
  await answer([A]);
  expect(mark()).toBeNull();
  expect(seen(".update-notice")).not.toBeNull();
});

test("with the list out, no composer shows, only the mark; a first run's empty list brings the welcome", async () => {
  const { answer } = await mount();
  await settle();
  expect(mark()).not.toBeNull();
  expect(mark()?.getAttribute("aria-hidden")).toBe("true");
  expect(mark()?.querySelector("svg path")).not.toBeNull();
  expect(deciding()).not.toBeNull();
  expect(seenComposer()).toBeNull();
  expect(welcome()).toBeNull();
  await answer([]);
  expect(welcome()).not.toBeNull();
  expect(mark()).toBeNull();
  expect(deciding()).toBeNull();
  expect(seenComposer()).toBeNull();
});

test("a list with a matter brings the room, with its composer, and the mark goes", async () => {
  const { answer } = await mount();
  await settle();
  expect(seenComposer()).toBeNull();
  expect(mark()).not.toBeNull();
  await answer([A]);
  expect(mark()).toBeNull();
  expect(deciding()).toBeNull();
  expect(welcome()).toBeNull();
  expect(seenComposer()).not.toBeNull();
});

test("a refused list shows the room at once, as before", async () => {
  vi.spyOn(console, "warn").mockImplementation(() => {});
  const { refuse } = await mount();
  await settle();
  expect(mark()).not.toBeNull();
  await refuse();
  expect(mark()).toBeNull();
  expect(deciding()).toBeNull();
  expect(seenComposer()).not.toBeNull();
});

test("a list that never comes leaves the mark up only DECIDE_MS, then the room shows as before", async () => {
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
  await mount();
  // past the 4–5 s a cold WebView2 took to build the window on the VM, the mark still stands
  expect(DECIDE_MS).toBe(15_000);
  await act(async () => vi.advanceTimersByTime(5000));
  expect(mark()).not.toBeNull();
  expect(seenComposer()).toBeNull();
  await act(async () => vi.advanceTimersByTime(DECIDE_MS - 5001));
  expect(mark()).not.toBeNull();
  expect(seenComposer()).toBeNull();
  await act(async () => vi.advanceTimersByTime(1));
  expect(mark()).toBeNull();
  expect(deciding()).toBeNull();
  expect(seenComposer()).not.toBeNull();
});
