// @vitest-environment happy-dom
// The room's title bar drags the window through its plan: the plan's meter carries the drag region on its four
// elements, and the plan's card, when it has one, is a press that does not drag.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, expect, test } from "vitest";
import { TooltipProvider } from "../ui/tooltip.tsx";
import { RoomScopeProvider } from "./room-scope-provider.tsx";
import { RoomToolbar, type RoomToolbarProps } from "./room-toolbar.tsx";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement | null = null;
let root: Root | null = null;

const toolbar: Omit<RoomToolbarProps, "plan"> = {
  title: "Acme Ltd v. State",
  titleHint: "Delhi High Court. CS 12/2026. Acme",
  nextDate: "3 March 2027",
  caseNo: "CS 12/2026",
  court: "Delhi High Court",
  history: [],
  canUndo: false,
  onUndo: () => {},
  railOpen: false,
  onRail: () => {},
  platform: "mac",
};

async function mount(plan: RoomToolbarProps["plan"], more: Partial<RoomToolbarProps> = {}) {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () =>
    root?.render(
      <TooltipProvider>
        <RoomScopeProvider className="room-window">
          <RoomToolbar {...toolbar} plan={plan} {...more} />
        </RoomScopeProvider>
      </TooltipProvider>,
    ),
  );
}

/** How many of the element and its descendants carry the drag region. */
const dragging = (el: Element | null | undefined) =>
  el === null || el === undefined
    ? 0
    : [el, ...el.querySelectorAll("*")].filter((e) => e.hasAttribute("data-tauri-drag-region"))
        .length;

afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  root = null;
  host = null;
});

test("the plan's meter in the title bar drags the window on its four elements", async () => {
  await mount({
    name: "Claude",
    left: 0.6,
    valueText: "60 percent of the plan left",
    status: "Plenty left",
  });
  const meter = host?.querySelector(".room-plan");
  expect(meter).not.toBeNull();
  expect(dragging(meter)).toBe(4);
});

test("the room's tools stay on one line, where the search folds instead of wrapping", async () => {
  await mount({
    name: "Claude",
    left: 0.6,
    valueText: "60 percent of the plan left",
    status: "Plenty left",
  });
  const bar = host?.querySelector(".room-tools-bar");
  expect(bar?.classList.contains("flex-nowrap")).toBe(true);
  expect(bar?.classList.contains("flex-wrap")).toBe(false);
});

test("a plan with a card keeps the drag region on the meter inside its press", async () => {
  await mount({
    name: "Claude",
    left: 0.5,
    valueText: "50 percent of the plan left",
    status: "Plenty left",
    cardLabel: "Show the plan's limits",
    card: <p>Every window</p>,
  });
  const press = host?.querySelector(".room-plan-press");
  expect(dragging(press)).toBe(4);
});

// The room's additive props: each leaves its part out only when asked, and draws today's toolbar at its default.
const parts = () => ({
  lights: host?.querySelector(".room-lights") ?? null,
  captions: host?.querySelector(".room-caption") ?? null,
  search: host?.querySelector(".room-search") ?? null,
  pair: host?.querySelector(".room-pair") ?? null,
  tools: host?.querySelector(".room-tools-bar") ?? null,
  title: host?.querySelector("h1") ?? null,
  top: host?.querySelector("header.room-top") ?? null,
});

test("by default the toolbar draws the lights on the Mac, the search field, the pair and the title in English", async () => {
  await mount(null);
  const p = parts();
  expect(p.lights).not.toBeNull();
  expect(p.search).not.toBeNull();
  expect(p.pair).not.toBeNull();
  expect(p.title?.getAttribute("lang")).toBe("en");
  expect(p.title?.classList.contains("as-en")).toBe(true);
  expect(p.title?.hasAttribute("data-testid")).toBe(false);
});

test("by default on Windows the toolbar ends in the caption buttons and keeps their room", async () => {
  await mount(null, { platform: "windows" });
  expect(parts().captions).not.toBeNull();
  expect(parts().top?.classList.contains("mid:pr-0")).toBe(true);
});

/** The toolbar's layout as the stylesheets key on it: each element's tag, classes and attributes, lights and caption
 * buttons left out. */
const layout = () =>
  [...(host?.querySelectorAll("header.room-top, header.room-top *") ?? [])]
    .filter((e) => e.closest(".room-lights, .room-caption") === null)
    .map((e) => `${e.tagName} ${e.className} ${[...e.attributes].map((a) => a.name).join(",")}`);

async function remount(more: Partial<RoomToolbarProps>) {
  await act(async () => root?.unmount());
  host?.remove();
  await mount(null, more);
}

test('on the Mac chrome "os" leaves out the corner lights and moves nothing else', async () => {
  await mount(null, { platform: "mac" });
  const own = layout();
  expect(parts().lights).not.toBeNull();
  await remount({ platform: "mac", chrome: "os" });
  expect(parts().lights).toBeNull();
  // the lead keeps its place, with Matters in it, and the title stands where it stood
  expect(host?.querySelector(".room-lead .room-rail-toggle")).not.toBeNull();
  expect(layout()).toEqual(own);
});

test('on Windows chrome "os" leaves out the caption buttons and their room at the right, and moves nothing else', async () => {
  await mount(null, { platform: "windows" });
  const own = layout();
  expect(parts().captions).not.toBeNull();
  await remount({ platform: "windows", chrome: "os" });
  const p = parts();
  expect(p.captions).toBeNull();
  expect(p.top?.getAttribute("data-platform")).toBe("windows");
  expect(p.top?.classList.contains("mid:pr-0")).toBe(false);
  expect(host?.querySelector(".room-tools")?.classList.contains("shrink-100")).toBe(false);
  // the caption buttons' room: the header's mid:pr-0, the tools' shrink-100 and the search's margins; nothing else
  const room = (line: string) =>
    line.replace(/ mid:pr-0| shrink-100| max-mid:ml-auto| max-narrow:ml-0/g, "");
  expect(layout().map(room)).toEqual(own.map(room));
});

test("search false leaves the search field out and keeps the pair", async () => {
  await mount(null, { search: false });
  expect(parts().search).toBeNull();
  expect(host?.querySelector("input")).toBeNull();
  expect(parts().pair).not.toBeNull();
});

test("versions false leaves the Versions and Undo pair out and keeps the search field", async () => {
  await mount(null, { versions: false });
  expect(parts().pair).toBeNull();
  expect(host?.querySelector(".room-undo")).toBeNull();
  expect(parts().search).not.toBeNull();
});

test("with neither search nor versions there is no empty toolbar group", async () => {
  await mount(null, { search: false, versions: false });
  expect(parts().tools).toBeNull();
  expect(host?.querySelector('[role="toolbar"]')).toBeNull();
});

test("testIds.title marks the title's heading", async () => {
  await mount(null, { testIds: { title: "matter" } });
  expect(host?.querySelector('[data-testid="matter"]')).toBe(parts().title);
});

test("titleLang sets the title's language for words that stand in a matter's name", async () => {
  await mount(null, { title: "अभी कोई मामला नहीं", titleLang: "hi" });
  expect(parts().title?.getAttribute("lang")).toBe("hi");
  expect(parts().title?.classList.contains("as-en")).toBe(false);
});
