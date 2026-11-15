// @vitest-environment happy-dom
// The sidebar's and the papers' toggles (room-layout.ts): from 1200 px each hides or shows its column and the window
// keeps the choice (layout-store.ts); below 1200 px the sidebar's toggle opens the folded rail's panel and leaves the
// kept choice alone; below 1000 px the papers' toggle hides the stacked papers; a panel hidden while it held the focus
// gives it to its toggle. Ctrl+B and Ctrl+Alt+B off the Mac, never with AltGr; on the Mac the View menu's items, whose
// keys the page leaves alone, so one press folds once; and the menu told the two shown flags.
import { act } from "react";
import { settled } from "@clauseo/components/lib/late.ts";
import { afterEach, beforeEach, expect, test } from "vitest";
import { detailed, fakeNext } from "./fake-next.ts";
import { readLayout } from "./layout-store.ts";
import { isLayoutKey, MENU_EVENT } from "./room-commands.ts";
import { mountRoom, THE_MATTER, type Harness } from "./thread/harness.ts";

let h: Harness | null = null;
let size = { width: 0, height: 0 };

/** The window's size, as media queries read it (happy-dom's viewport, not a stubbed innerWidth). */
const viewport = (width: number, height: number) =>
  (window as unknown as { happyDOM: { setViewport: (s: object) => void } }).happyDOM.setViewport({
    width,
    height,
  });

beforeEach(() => {
  size = { width: window.innerWidth, height: window.innerHeight };
  localStorage.clear();
});

afterEach(async () => {
  await h?.unmount();
  h = null;
  localStorage.clear();
  viewport(size.width, size.height);
});

const mount = async (at: number, platform: "mac" | "windows" = "windows") => {
  viewport(at, 900);
  h = await mountRoom({ platform, next: fakeNext({ matters: () => [detailed(THE_MATTER)] }) });
  return h;
};

const win = () => document.querySelector<HTMLElement>(".room-window");
const railToggle = () => document.querySelector<HTMLButtonElement>("[data-testid=rail-toggle]");
const wallToggle = () => document.querySelector<HTMLButtonElement>("[data-testid=wall-toggle]");
const rail = () => document.querySelector<HTMLElement>("nav.room-rail");
const pane = () => document.querySelector<HTMLElement>("aside.room-pane");
const nameOf = (b: HTMLButtonElement | null) =>
  b?.querySelector(".sr-only")?.textContent ?? b?.getAttribute("aria-label");

const press = async (b: HTMLButtonElement | null) => {
  await act(async () => b?.click());
  await h!.settle();
};
const menu = async (command: string) => {
  await act(
    async () => void window.dispatchEvent(new CustomEvent(MENU_EVENT, { detail: command })),
  );
  await act(async () => settled());
  await h!.settle();
};
const key = async (init: KeyboardEventInit, mods: string[] = []) => {
  const e = new KeyboardEvent("keydown", { bubbles: true, cancelable: true, ...init });
  // as a browser reports them: happy-dom answers AltGraph with altKey, which no browser does for a plain Alt
  e.getModifierState = (m: string) => mods.includes(m);
  await act(async () => void document.body.dispatchEvent(e));
  await h!.settle();
  return e.defaultPrevented;
};

test("at 1512 px the sidebar's toggle hides and shows the rail's column, says what it does, and the window keeps it", async () => {
  await mount(1512);
  expect(win()?.hasAttribute("data-rail")).toBe(false);
  expect(nameOf(railToggle())).toBe("Hide the sidebar");
  expect(railToggle()?.getAttribute("aria-expanded")).toBe("true");
  expect(railToggle()?.getAttribute("aria-controls")).toBe(rail()?.id);
  await press(railToggle());
  expect(win()?.getAttribute("data-rail")).toBe("hidden");
  expect(nameOf(railToggle())).toBe("Show the sidebar");
  expect(railToggle()?.getAttribute("aria-expanded")).toBe("false");
  expect(readLayout("main")).toEqual({ sidebar: false, papers: true });
  // a new room in the window starts as it was left
  await h!.unmount();
  await mount(1512);
  expect(win()?.getAttribute("data-rail")).toBe("hidden");
  await press(railToggle());
  expect(win()?.hasAttribute("data-rail")).toBe(false);
  expect(readLayout("main")).toEqual({ sidebar: true, papers: true });
});

test("at 1512 px the papers' toggle hides and shows the wall, and the window keeps it", async () => {
  await mount(1512);
  expect(nameOf(wallToggle())).toBe("Hide the papers");
  expect(wallToggle()?.getAttribute("aria-controls")).toBe(pane()?.id);
  expect(wallToggle()?.closest(".room-tools")).not.toBeNull();
  await press(wallToggle());
  expect(win()?.getAttribute("data-wall")).toBe("hidden");
  expect(nameOf(wallToggle())).toBe("Show the papers");
  expect(wallToggle()?.getAttribute("aria-expanded")).toBe("false");
  expect(readLayout("main")).toEqual({ sidebar: true, papers: false });
  await press(wallToggle());
  expect(win()?.hasAttribute("data-wall")).toBe(false);
});

test("at 1100 px the sidebar's toggle opens the folded rail's panel and leaves the kept choice alone", async () => {
  await mount(1100);
  expect(nameOf(railToggle())).toBe("Show the sidebar");
  expect(railToggle()?.getAttribute("aria-expanded")).toBe("false");
  await press(railToggle());
  expect(rail()?.hasAttribute("data-open")).toBe(true);
  expect(nameOf(railToggle())).toBe("Hide the sidebar");
  expect(win()?.hasAttribute("data-rail")).toBe(false);
  expect(localStorage.getItem("clauseo.layout.main")).toBeNull();
  await press(railToggle());
  expect(rail()?.hasAttribute("data-open")).toBe(false);
  expect(localStorage.getItem("clauseo.layout.main")).toBeNull();
});

test("at 900 px the papers' toggle hides the stacked papers", async () => {
  await mount(900);
  await press(wallToggle());
  expect(win()?.getAttribute("data-wall")).toBe("hidden");
  expect(readLayout("main")).toEqual({ sidebar: true, papers: false });
});

test("a panel hidden while it holds the focus gives the focus to its toggle", async () => {
  await mount(1512, "mac");
  const row = rail()?.querySelector<HTMLElement>("[data-testid=matter-row]");
  expect(row).not.toBeNull();
  await act(async () => row?.focus());
  await menu("toggle_sidebar");
  expect(win()?.getAttribute("data-rail")).toBe("hidden");
  expect(document.activeElement).toBe(railToggle());
  const inPane = document.createElement("button");
  pane()?.append(inPane);
  await act(async () => inPane.focus());
  await menu("toggle_papers");
  expect(win()?.getAttribute("data-wall")).toBe("hidden");
  expect(document.activeElement).toBe(wallToggle());
});

test("on Windows Ctrl+B folds the sidebar and Ctrl+Alt+B the papers, never with AltGr", async () => {
  await mount(1512);
  expect(await key({ key: "b", code: "KeyB", ctrlKey: true })).toBe(true);
  expect(win()?.getAttribute("data-rail")).toBe("hidden");
  expect(await key({ key: "b", code: "KeyB", ctrlKey: true, altKey: true })).toBe(true);
  expect(win()?.getAttribute("data-wall")).toBe("hidden");
  // AltGr, as Ctrl+Alt: a character on the layout, never the papers' key
  expect(await key({ key: "ब", code: "KeyB", ctrlKey: true, altKey: true }, ["AltGraph"])).toBe(
    false,
  );
  expect(win()?.getAttribute("data-wall")).toBe("hidden");
  // the letter's place on a layout with no Latin letter
  expect(await key({ key: "ब", code: "KeyB", ctrlKey: true })).toBe(true);
  expect(win()?.hasAttribute("data-rail")).toBe(false);
});

test("on the Mac the page leaves ⌃⌘S and ⌥⌘I to the View menu, so one press folds once", async () => {
  await mount(1512, "mac");
  // the page hears the key first and leaves it: the menu's accelerator then sends its command
  expect(await key({ key: "s", code: "KeyS", ctrlKey: true, metaKey: true })).toBe(false);
  expect(await key({ key: "ˆ", code: "KeyI", altKey: true, metaKey: true })).toBe(false);
  expect(await key({ key: "b", code: "KeyB", ctrlKey: true })).toBe(false);
  expect(win()?.hasAttribute("data-rail")).toBe(false);
  expect(win()?.hasAttribute("data-wall")).toBe(false);
  await menu("toggle_sidebar");
  expect(win()?.getAttribute("data-rail")).toBe("hidden");
  await menu("toggle_papers");
  expect(win()?.getAttribute("data-wall")).toBe("hidden");
});

test("the Mac's View menu is told the two shown flags; below 1200 px the sidebar's is its panel's", async () => {
  await mount(1100, "mac");
  const told: [boolean, boolean][] = [];
  h!.transport.layoutMenu = async (sidebar, papers) => void told.push([sidebar, papers]);
  await press(railToggle());
  expect(told.at(-1)).toEqual([true, true]);
  await press(wallToggle());
  expect(told.at(-1)).toEqual([true, false]);
  await press(railToggle());
  expect(told.at(-1)).toEqual([false, false]);
});

test("the layout keys: Ctrl+B and Ctrl+Alt+B off the Mac only, by the key's place, AltGr and Shift never", () => {
  const k = (init: KeyboardEventInit, altGraph = false) => {
    const e = new KeyboardEvent("keydown", init);
    e.getModifierState = (m: string) => altGraph && m === "AltGraph";
    return e;
  };
  expect(isLayoutKey(k({ key: "b", code: "KeyB", ctrlKey: true }), false, false)).toBe(true);
  expect(isLayoutKey(k({ key: "b", code: "KeyB", ctrlKey: true, altKey: true }), true, false)).toBe(
    true,
  );
  expect(
    isLayoutKey(k({ key: "b", code: "KeyB", ctrlKey: true, altKey: true }), false, false),
  ).toBe(false);
  expect(
    isLayoutKey(k({ key: "b", code: "KeyB", ctrlKey: true, altKey: true }, true), true, false),
  ).toBe(false);
  expect(
    isLayoutKey(k({ key: "B", code: "KeyB", ctrlKey: true, shiftKey: true }), false, false),
  ).toBe(false);
  expect(isLayoutKey(k({ key: "b", code: "KeyB", metaKey: true }), false, true)).toBe(false);
  expect(isLayoutKey(k({ key: "b", code: "KeyB", ctrlKey: true }), false, true)).toBe(false);
  expect(isLayoutKey(k({ key: "n", code: "KeyN", ctrlKey: true }), false, false)).toBe(false);
});
