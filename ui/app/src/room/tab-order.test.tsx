// @vitest-environment happy-dom
// D-049: Tab through the room. From the window's start the focus goes through the toolbar, the rail (New matter, Find,
// Matters / Recent, every matter and the open matter's conversations, New conversation, the scratch matter,
// Settings), the composer and the papers, then round again, and never drops to the page itself: Tab out of an empty
// Find lands on Matters / Recent, not on a row of Find's own list as that list goes (which WebKit followed by starting
// again at the page's top, a loop that never reached the rest). Each stop is a control a browser draws the room's
// ring on (base.css's :focus-visible, or its field's :has(:focus-visible)). And axe over the room with Find's focus.
// happy-dom has no Tab of its own, so `tab` plays the browser's: the key goes to the focused control first, and unless
// it was prevented the focus moves to the next control in the DOM as it stands after the key's handlers ran, from the
// page's start when nothing has it.
import axe from "axe-core";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { settled } from "@clauseo/components/lib/late.ts";
import { afterEach, expect, test } from "vitest";
import { FakeTransport } from "../transport/fake-transport.ts";
import type { MatterSummary } from "../transport/protocol.ts";
import { uuid } from "../testing/stream-script.ts";
import { fakeNext, railStates, railThreads } from "./fake-next.ts";
import type { PaperSummary } from "./host-next.ts";
import { frameBatch } from "../store/batch.ts";
import { StoreContext } from "../store/hooks.ts";
import { createViewStore } from "../store/store.ts";
import { Room } from "./room.tsx";
import { stubFonts } from "./thread/testing.ts";
import { connectRoom } from "./start.ts";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
stubFonts();

let host: HTMLDivElement | null = null;
let root: Root | null = null;

// the page's own title, as index.html gives it, for axe's document-title
document.title = "Clauseo";

afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  root = null;
  host = null;
});

const OPEN: MatterSummary = {
  matter: uuid(1, 2),
  name: "Sharma v. State of Delhi",
  kind: "folder",
};
const SCRATCH: MatterSummary = { matter: uuid(99, 2), name: "Scratch", kind: "scratch" };
const PAPERS: PaperSummary[] = ["Notice to the bank.pdf", "Board minutes.pdf"].map((name, i) => ({
  file: uuid(i + 1, 8),
  path: name,
  size: 1000,
  modified: 1_730_000_000_000,
}));

/** The room with every rail state (fake-next.ts), the open matter's conversations, its papers and Settings. */
async function mount() {
  const now = Date.now();
  const transport = new FakeTransport({
    openSettings: () => {},
    query: () => ({ matters: [] }),
    next: fakeNext({
      matters: () => railStates(OPEN, SCRATCH, now),
      papers: PAPERS,
      frames: true,
      threads: railThreads(OPEN, SCRATCH, now),
    }),
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
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 0));
    await settled();
  });
}

const TABBABLE =
  'a[href], button:not([disabled]), input:not([disabled]):not([type="hidden"]), textarea:not([disabled]), select:not([disabled]), [tabindex]';

/** The controls Tab stops at, in the DOM's order: not disabled, not tabindex -1, not hidden or inert. Matters, the
 * toolbar's button for the folded rail, is left out: responsive.css shows it only below 1200 px, and happy-dom lays
 * nothing out, so the test stands for a wide window. */
function tabbables(): HTMLElement[] {
  return [...document.querySelectorAll<HTMLElement>(TABBABLE)].filter(
    (el) =>
      el.tabIndex >= 0 &&
      !el.matches(":disabled, .room-rail-toggle") &&
      el.getAttribute("tabindex") !== "-1" &&
      el.closest("[hidden], [inert], [aria-hidden='true']") === null,
  );
}

/** One Tab, as a browser plays it (module docs). */
async function tab() {
  const from =
    document.activeElement instanceof HTMLElement ? document.activeElement : document.body;
  let prevented = false;
  await act(async () => {
    const key = new KeyboardEvent("keydown", { key: "Tab", bubbles: true, cancelable: true });
    from.dispatchEvent(key);
    prevented = key.defaultPrevented;
  });
  if (prevented) return;
  const all = tabbables();
  const start = from === document.body || !from.isConnected ? -1 : all.indexOf(from);
  const next =
    start === -1 && from !== document.body && from.isConnected
      ? all.find((el) => from.compareDocumentPosition(el) & Node.DOCUMENT_POSITION_FOLLOWING)
      : all[start + 1];
  await act(async () => (next ?? all[0])?.focus());
  // what a blur or focus changed lands in the same press
  await act(async () => new Promise((resolve) => setTimeout(resolve, 0)));
}

/** A stop's name, as the test reads it: its label, its placeholder or its words. */
function nameOf(el: Element | null): string {
  if (el === null || el === document.body) return "<page>";
  const label = el.getAttribute("aria-label") ?? el.getAttribute("placeholder");
  return (label ?? el.textContent ?? "").replace(/\s+/g, " ").trim();
}

/** Whether `el` draws a ring when focused from the keyboard: base.css's global :focus-visible outline, unless its own
 * class takes it off; then its field draws it (Find's box, the composer's field). */
function ringed(el: HTMLElement): boolean {
  if (el.matches("input, textarea"))
    return el.closest(".room-find, .room-field, label, [class*='is-focus-within']") !== null;
  return !/(^|\s)(outline-none|is-focus:outline-none)(\s|$)/.test(el.className);
}

test("Tab goes through the toolbar, the whole rail, the composer and the papers, and round again, never to the page", async () => {
  await mount();
  const seen: string[] = [];
  const stops: HTMLElement[] = [];
  // from the window's start: nothing focused
  (document.activeElement as HTMLElement | null)?.blur();
  for (let i = 0; i < 80; i += 1) {
    await tab();
    const at = document.activeElement;
    expect(at, `press ${i + 1}, after ${seen.at(-1)}`).not.toBe(document.body);
    expect(at?.isConnected, `press ${i + 1}`).toBe(true);
    seen.push(nameOf(at));
    stops.push(at as HTMLElement);
  }
  // one round: from the first stop to its return
  const round = seen.indexOf(seen[0]!, 1);
  expect(round, seen.join(" → ")).toBeGreaterThan(0);
  const once = seen.slice(0, round);
  // in reading order: the toolbar, then the rail top to foot, then the composer, then the papers
  const order = [
    "Versions",
    "New matter",
    "Find",
    "Matters",
    "Recent",
    // the matters, the open one among them (the fake data's names, matched by their start)
    "Kapoor Exports",
    "Sharma v. State of Delhi",
    "Actions for Sharma v. State of Delhi",
    "New conversation",
    "Chopra Steel",
    "Scratch matter",
    "Settings",
    "Ask about this matter",
    "Notice to the bank.pdf",
    "Board minutes.pdf",
  ];
  const at = order.map((name) => once.findIndex((s) => s === name || s.startsWith(name)));
  expect(
    order.filter((_, i) => at[i] === -1),
    once.join(" → "),
  ).toEqual([]);
  expect(at, once.join(" → ")).toEqual([...at].sort((a, b) => a - b));
  // the open matter's conversations stand between its row and New conversation
  const conversations = once.slice(at[7]! + 1, at[8]!);
  expect(conversations.length, once.join(" → ")).toBeGreaterThan(0);
  // every stop draws the ring
  expect(
    stops
      .slice(0, round)
      .filter((s) => !ringed(s))
      .map(nameOf),
  ).toEqual([]);
});

test("Tab out of an empty Find lands on Matters / Recent, which stay", async () => {
  await mount();
  const find = document.querySelector<HTMLInputElement>('input[type="search"]');
  await act(async () => find?.focus());
  // Find's own list stands while it has the focus: Matters / Recent are hidden
  expect(document.querySelector('[data-testid="rail-switch"]')).toBeNull();
  await tab();
  expect(nameOf(document.activeElement)).toBe("Matters");
  expect(document.activeElement?.closest('[data-testid="rail-switch"]')).not.toBeNull();
  await tab();
  expect(nameOf(document.activeElement)).toBe("Recent");
});

test("axe finds no violation in the room with Find's focus, nor after Tab leaves it", async () => {
  await mount();
  const find = document.querySelector<HTMLInputElement>('input[type="search"]');
  await act(async () => find?.focus());
  const during = await axe.run(document, { resultTypes: ["violations"] });
  expect(during.violations.map((v) => v.id)).toEqual([]);
  await tab();
  const after = await axe.run(document, { resultTypes: ["violations"] });
  expect(after.violations.map((v) => v.id)).toEqual([]);
});
