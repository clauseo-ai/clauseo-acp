// @vitest-environment happy-dom
// Where the composer's menu stands (composer-menu-place.ts): the rule apart from the page, then through the composer
// with stubbed boxes (happy-dom lays nothing out): bound by the column that clips it, 4 px above the field, the lit row
// scrolled into the rows' view by the rows alone, and placed again, once a frame, as the field's own size changes.
import { act, StrictMode, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, test, vi } from "vitest";
import { settled } from "../lib/late.ts";
import { DEFAULT_DISPLAY, showDisplay } from "../lib/display.ts";
import { TooltipProvider } from "../ui/tooltip.tsx";
import { Composer, type ComposerProps, type SlashCommand } from "./composer.tsx";
import { placeMenu } from "./composer-menu-place.ts";
import { RoomScopeProvider } from "./room-scope-provider.tsx";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
if (!("fonts" in document)) {
  Object.defineProperty(document, "fonts", {
    value: { ready: Promise.resolve(), status: "loaded", load: async () => [] },
  });
}

const COMMANDS: SlashCommand[] = [
  { name: "review", description: "Review the changes" },
  { name: "restart", description: "Start the session again" },
  { name: "compact", description: "Compact the conversation", input_hint: "what to keep" },
];

let host: HTMLDivElement | null = null;
let root: ReturnType<typeof createRoot> | null = null;

const wrap = (node: ReactNode) => (
  <StrictMode>
    <TooltipProvider>
      <RoomScopeProvider>{node}</RoomScopeProvider>
    </TooltipProvider>
  </StrictMode>
);

const IDS = {
  field: "composer",
  ask: "send",
  menu: "composer-menu",
  menuItem: "composer-menu-item",
};
const composer = (p: Partial<ComposerProps>) => (
  <Composer took={null} plan="" hint={null} testIds={IDS} commands={COMMANDS} {...p} />
);

async function mount(node: ReactNode) {
  showDisplay({ ...DEFAULT_DISPLAY, motion: "reduced" });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root?.render(wrap(node)));
}

afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  root = null;
  showDisplay(DEFAULT_DISPLAY);
});

const field = () => document.querySelector<HTMLTextAreaElement>('[data-testid="composer"]')!;
const menu = () => document.querySelector<HTMLElement>('[data-testid="composer-menu"]');
const lit = () =>
  [...document.querySelectorAll<HTMLElement>('[data-testid="composer-menu-item"]')].find(
    (r) => r.getAttribute("aria-selected") === "true",
  );

/** The field focused, and the menu's chunk in. */
const focus = async () => {
  await act(async () => field().focus());
  await act(settled);
};

/** An edit as an engine makes it: the new value with the caret at its end, then the input event. */
const edit = (value: string) =>
  act(async () => {
    const a = field();
    Reflect.set(HTMLTextAreaElement.prototype, "value", value, a);
    a.setSelectionRange(value.length, value.length);
    a.dispatchEvent(new InputEvent("input", { bubbles: true, inputType: "insertText" }));
  });

/** A key in the field; true when its default was prevented. */
const key = (k: string) =>
  act(async () => {
    const e = new KeyboardEvent("keydown", { key: k, bubbles: true, cancelable: true });
    field().dispatchEvent(e);
    return e.defaultPrevented;
  });

test("the menu stands at the caret's x, below the field, or above it at the window's foot, inside what clips it", () => {
  const view = { left: 0, top: 0, right: 1000, bottom: 600 };
  // room below: 4 px under the line
  expect(placeMenu({ x: 120, top: 100, bottom: 126 }, { width: 240, height: 200 }, view)).toEqual({
    left: 120,
    top: 130,
    maxHeight: 600 - 8 - 130,
  });
  // the composer at the window's foot: 4 px above the line, as tall as the room above allows
  expect(placeMenu({ x: 120, top: 540, bottom: 566 }, { width: 240, height: 200 }, view)).toEqual({
    left: 120,
    top: 540 - 4 - 200,
    maxHeight: 540 - 4 - 8,
  });
  // a caret near the right edge: the menu keeps 8 px inside
  expect(placeMenu({ x: 900, top: 100, bottom: 126 }, { width: 240, height: 100 }, view).left).toBe(
    1000 - 8 - 240,
  );
  // a menu taller than either side: the larger side, cut to it
  const tall = placeMenu({ x: 10, top: 200, bottom: 226 }, { width: 240, height: 900 }, view);
  expect(tall).toEqual({ left: 10, top: 230, maxHeight: 600 - 8 - 230 });
  // the room's column clips from y=64: the room above the field runs from its top, 8 px in, and the menu ends 4 px
  // above the field; at the column's left, 8 px in from it
  const column = { left: 300, top: 64, right: 1000, bottom: 588 };
  expect(placeMenu({ x: 290, top: 500, bottom: 560 }, { width: 240, height: 900 }, column)).toEqual(
    {
      left: 308,
      top: 72,
      maxHeight: 500 - 4 - 72,
    },
  );
});

test("near the room column's right edge the menu keeps 8 px inside the column, left of the caret (#117 M2-ta3, WKWebView)", () => {
  // the run's numbers: the column (.room-centre) ends at 935.6 px in a 1280 px window and at 794 px once 100 px
  // narrower; the menu is 262 px wide; the caret on line 3 of the Tamil text stood at 711.1, then 552.4. The menu
  // stood at 666, then 524: the column's clamp, not a caret measured wrong (the run's check knew only the window's)
  const line = { top: 739, bottom: 765 };
  const wide = { left: 200, top: 0, right: 935.6, bottom: 800 };
  const narrow = { left: 12, top: 0, right: 794, bottom: 800 };
  expect(placeMenu({ x: 711.1, ...line }, { width: 262, height: 412 }, wide).left).toBe(666);
  expect(placeMenu({ x: 552.4, ...line }, { width: 262, height: 412 }, narrow).left).toBe(524);
  // short of the clamp, at the caret
  expect(placeMenu({ x: 600, ...line }, { width: 262, height: 412 }, wide).left).toBe(600);
});

test("the open menu is placed again as the field's own size changes, before that frame paints; closed, it watches no more", async () => {
  // the field's size observers, and the frames they ask for, held until the test runs them
  const watching: { cb: ResizeObserverCallback; targets: Element[]; gone: boolean }[] = [];
  vi.stubGlobal(
    "ResizeObserver",
    class {
      seen: { cb: ResizeObserverCallback; targets: Element[]; gone: boolean };
      constructor(cb: ResizeObserverCallback) {
        this.seen = { cb, targets: [], gone: false };
        watching.push(this.seen);
      }
      observe(t: Element) {
        this.seen.targets.push(t);
      }
      unobserve() {}
      disconnect() {
        this.seen.gone = true;
      }
    },
  );
  const frames = new Map<number, FrameRequestCallback>();
  let next = 0;
  vi.stubGlobal("requestAnimationFrame", (cb: FrameRequestCallback) => {
    frames.set(++next, cb);
    return next;
  });
  vi.stubGlobal("cancelAnimationFrame", (id: number) => frames.delete(id));
  // the field's drawn box: 3 lines, then 4 as the window narrows and the field grows
  let tall = 80;
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (
    this: HTMLElement,
  ) {
    return this.classList.contains("room-field")
      ? new DOMRect(0, 40, 600, tall)
      : new DOMRect(0, 0, 0, 0);
  });
  // the plane the list stands in, which the menu places
  const top = () => menu()?.closest<HTMLElement>(".fixed")?.style.top;
  try {
    await mount(composer({}));
    await focus();
    const before = watching.length;
    await edit("/");
    expect(top()).toBe("124px");
    const mine = watching.slice(before).find((o) => o.targets.includes(field()));
    expect(mine).toBeDefined();
    const grow = () => act(async () => mine?.cb([], {} as ResizeObserver));
    // the field grows a line: placed again in the observer's own callback, which runs before the frame paints, with no
    // frame of its own between (one such frame showed the grown field under the menu)
    tall = 106;
    await grow();
    expect(frames.size).toBe(0);
    expect(top()).toBe("150px");
    // closed: the observer is let go
    expect(await key("Escape")).toBe(true);
    expect(menu()).toBeNull();
    expect(mine?.gone).toBe(true);
  } finally {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  }
});

test("a menu taller than the room above the field is bound by the column that clips it, 4 px above the field; its rows alone scroll the lit one into view", async () => {
  const many: SlashCommand[] = Array.from({ length: 20 }, (_, i) => ({
    name: `step${String(i).padStart(2, "0")}`,
    description: "",
  }));
  // the room's centre column clips from y=64 to 764; the field's box stands at 600 to 660; the rows are 48 px each,
  // under the list's top at 80, its foot at 580, moved by its scroll
  const rect = function (this: HTMLElement) {
    if (this.tagName === "MAIN") return new DOMRect(0, 64, 1280, 700);
    if (this.classList.contains("room-field")) return new DOMRect(0, 600, 800, 60);
    if (this.getAttribute("role") === "listbox") return new DOMRect(0, 80, 300, 500);
    if (this.getAttribute("role") === "option") {
      const list = this.parentElement!;
      const i = [...list.children].indexOf(this);
      return new DOMRect(0, 80 + i * 48 - list.scrollTop, 300, 48);
    }
    return new DOMRect(0, 0, 0, 0);
  };
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(rect);
  // the plane's own height, unbound: all twenty rows
  vi.spyOn(HTMLElement.prototype, "offsetHeight", "get").mockImplementation(function (
    this: HTMLElement,
  ) {
    return this.classList.contains("fixed") ? 20 * 48 : 0;
  });
  const into = vi.fn();
  Object.defineProperty(Element.prototype, "scrollIntoView", { value: into, configurable: true });
  const plane = () => menu()?.closest<HTMLElement>(".fixed");
  try {
    await mount(<main style={{ overflow: "hidden" }}>{composer({ commands: many })}</main>);
    await focus();
    await edit("/");
    expect(plane()?.parentElement?.tagName).toBe("MAIN");
    // from the column's top, 8 px in, to 4 px above the field: not from the window's top
    expect(plane()?.style.maxHeight).toBe(`${600 - 4 - (64 + 8)}px`);
    expect(plane()?.style.top).toBe("72px");
    // the lit row, the last after ArrowUp, is scrolled into the rows' view, and back to the first
    const list = menu()!;
    expect(list.scrollTop).toBe(0);
    await key("ArrowUp");
    expect(lit()?.textContent).toContain("/step19");
    expect(list.scrollTop).toBe(80 + 19 * 48 + 48 - 580);
    await key("ArrowUp");
    expect(list.scrollTop).toBe(80 + 19 * 48 + 48 - 580);
    await key("ArrowDown");
    await key("ArrowDown");
    expect(lit()?.textContent).toContain("/step00");
    expect(list.scrollTop).toBe(0);
    // never through scrollIntoView, which would scroll the clipping column too
    expect(into).not.toHaveBeenCalled();
  } finally {
    vi.restoreAllMocks();
    Reflect.deleteProperty(Element.prototype, "scrollIntoView");
  }
});
