// @vitest-environment happy-dom
// The room's title bar drags the window through its plan: a plan without a card carries the drag region on its meter's
// four elements, and the plan's card, when it has one, is a press that does not drag.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, expect, test, vi } from "vitest";
import { TooltipProvider } from "../ui/tooltip.tsx";
import { RoomScopeProvider } from "./room-scope-provider.tsx";
import { DEFAULT_DISPLAY, showDisplay } from "../lib/display.ts";
import { sentences } from "../lib/sentences.ts";
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

test("a plan with a card has no drag region: a press on it opens the card, never drags the window", async () => {
  await mount({
    name: "Claude",
    left: 0.5,
    valueText: "50 percent of the plan left",
    status: "Plenty left",
    cardLabel: "Show the plan's limits",
    card: <p>Every window</p>,
  });
  const press = host?.querySelector(".room-plan-press");
  expect(press).not.toBeNull();
  expect(dragging(press)).toBe(0);
});

test("a stale plan with a card mutes its fill, inside the press where the meter has no role; a fresh one doesn't", async () => {
  const stale = {
    name: "Claude",
    left: 0.4,
    valueText: "40 percent of the plan left",
    status: "Plenty left",
    look: "stale" as const,
    cardLabel: "Show the plan's limits",
    card: <p>Every window</p>,
  };
  await mount(stale);
  const fill = () => host?.querySelector(".room-plan-press .rounded-meter > *");
  expect(host?.querySelector('.room-plan-press [role="meter"]')).toBeNull();
  expect(fill()?.hasAttribute("data-muted")).toBe(true);
  expect(fill()?.classList.contains("bg-flat-num")).toBe(true);
  expect(fill()?.classList.contains("bg-primary")).toBe(false);
  await act(async () => root?.unmount());
  host?.remove();
  const { look: _, ...fresh } = stale;
  await mount(fresh);
  expect(fill()?.hasAttribute("data-muted")).toBe(false);
  expect(fill()?.classList.contains("bg-primary")).toBe(true);
});

test("the plan's press says its parts as sentences in the language: the danda in Hindi, no empty part", async () => {
  expect(
    sentences("en", [
      "Claude plan",
      "Plenty left",
      "70 percent of the plan left",
      "Show the plan's limits",
    ]),
  ).toBe("Claude plan. Plenty left. 70 percent of the plan left. Show the plan's limits");
  expect(sentences("hi", ["Claude प्लान", "काफ़ी बाकी है", "", "प्लान की सीमाएँ दिखाएँ"])).toBe(
    "Claude प्लान। काफ़ी बाकी है। प्लान की सीमाएँ दिखाएँ",
  );
  // a part that ends in its stop keeps it, never doubled
  expect(sentences("hi", ["सीमा के करीब। 6 बजे रीसेट होगा।", "प्लान का 20 प्रतिशत बाकी है"])).toBe(
    "सीमा के करीब। 6 बजे रीसेट होगा। प्लान का 20 प्रतिशत बाकी है",
  );
  showDisplay({ ...DEFAULT_DISPLAY, lang: "hi" });
  try {
    await mount({
      name: "Claude",
      left: 0.7,
      valueText: "प्लान का 70 प्रतिशत बाकी है",
      status: "काफ़ी बाकी है",
      cardLabel: "प्लान की सीमाएँ दिखाएँ",
      card: <p>Every window</p>,
    });
    const press = host?.querySelector(".room-plan-press");
    expect(press?.getAttribute("aria-label")).toBe(
      "Claude प्लान। काफ़ी बाकी है। प्लान का 70 प्रतिशत बाकी है। प्लान की सीमाएँ दिखाएँ",
    );
    expect(press?.getAttribute("aria-label")).not.toMatch(/\. /);
  } finally {
    showDisplay(DEFAULT_DISPLAY);
  }
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

test("an empty title draws no heading, never an empty one, and keeps the line under it", async () => {
  await mount(null, { title: "", testIds: { title: "matter" } });
  expect(host?.querySelector("h1")).toBeNull();
  expect(host?.querySelector('[data-testid="matter"]')).toBeNull();
  expect(host?.querySelector(".room-matter .room-next")).not.toBeNull();
  await remount({ title: "Acme Ltd v. State" });
  expect(host?.querySelector("h1")?.textContent).toBe("Acme Ltd v. State");
});

test("the caption wiring reaches the buttons: each press calls its handler, and maximised shows Restore", async () => {
  const caption = {
    maximised: true,
    onMinimise: vi.fn(),
    onToggleMaximise: vi.fn(),
    onClose: vi.fn(),
  };
  await mount(null, { platform: "windows", caption });
  const max = host?.querySelector<HTMLButtonElement>('[data-caption="maximise"]');
  expect(max?.getAttribute("aria-label")).toBe("Restore");
  const [min, , close] = [...(parts().captions?.querySelectorAll("button") ?? [])];
  await act(async () => min?.click());
  await act(async () => max?.click());
  await act(async () => close?.click());
  expect(caption.onMinimise).toHaveBeenCalledTimes(1);
  expect(caption.onToggleMaximise).toHaveBeenCalledTimes(1);
  expect(caption.onClose).toHaveBeenCalledTimes(1);
});

test("nativeLights marks the corner lights for macOS's own, and only then", async () => {
  await mount(null, { platform: "mac", nativeLights: true });
  expect(parts().lights?.hasAttribute("data-native")).toBe(true);
  await remount({ platform: "mac" });
  expect(parts().lights?.hasAttribute("data-native")).toBe(false);
});

test('chrome "os" draws neither the lights nor the buttons, wiring or not', async () => {
  const caption = { maximised: false, onClose: vi.fn() };
  await mount(null, { platform: "mac", chrome: "os", nativeLights: true, caption });
  expect(parts().lights).toBeNull();
  await remount({ platform: "windows", chrome: "os", nativeLights: true, caption });
  expect(parts().captions).toBeNull();
});
