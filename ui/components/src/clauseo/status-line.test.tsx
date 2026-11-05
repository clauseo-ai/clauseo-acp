// @vitest-environment happy-dom
// The status line's tool segment: after the plan, parted by the dot, and inside the line's fit, so a tool phrase that
// leaves no room wraps the line to two rather than cutting anything short, measured again when the phrase changes.
import { act, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { afterAll, afterEach, expect, test } from "vitest";
import { DEFAULT_DISPLAY, showDisplay } from "../lib/display.ts";
import { RoomScopeProvider } from "./room-scope-provider.tsx";
import { StatusLine, type ToolsSegment } from "./status-line.tsx";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement | null = null;
let root: ReturnType<typeof createRoot> | null = null;
/** The line's words' width, as the engine would lay them out: a phrase longer than `room` overflows the line. */
let room = 1000;
/** Set, the line is measured by `layout` instead: the status box this wide, the hint taking its share of it. */
let boxWidth: number | null = null;

const proto = HTMLElement.prototype;
const scroll = Object.getOwnPropertyDescriptor(proto, "scrollWidth");
const client = Object.getOwnPropertyDescriptor(proto, "clientWidth");
Object.defineProperty(proto, "scrollWidth", {
  configurable: true,
  get(this: HTMLElement) {
    if (!this.classList.contains("room-status-line")) return 0;
    if (boxWidth !== null) return layout(this.closest<HTMLElement>(".room-status")!).content;
    // wrapped, the line takes two and nothing stands past it
    if (this.closest<HTMLElement>(".room-status")?.dataset["fit"] === "wrapped") return 0;
    return (this.textContent ?? "").length * 8;
  },
});
Object.defineProperty(proto, "clientWidth", {
  configurable: true,
  get(this: HTMLElement) {
    if (!this.classList.contains("room-status-line")) return 0;
    if (boxWidth !== null) return layout(this.closest<HTMLElement>(".room-status")!).line;
    return room;
  },
});

afterAll(() => {
  if (scroll !== undefined) Object.defineProperty(proto, "scrollWidth", scroll);
  if (client !== undefined) Object.defineProperty(proto, "clientWidth", client);
});

afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  root = null;
  room = 1000;
  boxWidth = null;
});

const segment = (summary: string): ToolsSegment => ({
  summary,
  node: <button type="button">{summary}</button>,
});

const wrap = (node: ReactNode) => <RoomScopeProvider>{node}</RoomScopeProvider>;

async function mount(node: ReactNode) {
  showDisplay({ ...DEFAULT_DISPLAY, motion: "reduced" });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root?.render(wrap(node)));
}
const rerender = (node: ReactNode) => act(async () => root?.render(wrap(node)));

const status = () => host!.querySelector<HTMLElement>(".room-status");
const tools = () => host!.querySelector<HTMLElement>(".room-status-tools");

test("the tool segment follows the plan, parted by the dot, as the line's last part", async () => {
  await mount(
    <StatusLine
      took="Took 38 seconds"
      plan="Uses your Claude plan"
      hint={null}
      tools={segment("Web search on")}
    />,
  );
  const line = host!.querySelector(".room-status-line");
  expect(line?.lastElementChild).toBe(tools());
  expect(tools()?.firstElementChild?.className).toBe("room-dot");
  expect(tools()?.textContent).toBe("Web search on");
  expect(status()?.dataset["fit"]).toBe("full");
});

test("with nothing before it the segment stands alone, with no dot; with none there is no segment", async () => {
  await mount(<StatusLine took={null} plan="" hint={null} tools={segment("Browser off")} />);
  expect(tools()?.querySelector(".room-dot")).toBeNull();
  await rerender(<StatusLine took={null} plan="Uses your Claude plan" hint={null} />);
  expect(tools()).toBeNull();
});

test("a phrase that leaves no room wraps the line, cutting nothing, and the fit runs again as the phrase changes", async () => {
  room = 400;
  const plan = "Uses your Claude plan";
  await mount(<StatusLine took={null} plan={plan} hint={null} tools={segment("Browser on")} />);
  expect(status()?.dataset["fit"]).toBe("full");
  await rerender(
    <StatusLine
      took="Took 38 seconds"
      plan={plan}
      hint={null}
      tools={segment("Web search and browser on")}
    />,
  );
  expect(status()?.dataset["fit"]).toBe("wrapped");
  expect(tools()?.textContent).toBe("Web search and browser on");
  await rerender(<StatusLine took={null} plan={plan} hint={null} tools={segment("Browser on")} />);
  expect(status()?.dataset["fit"]).toBe("full");
});

/* ---------- a deterministic layout, for the fit's boxes ---------- */

const CH = 8; // every letter 8 px wide
const GAP = 8; // the line's and the hint's gap
const DOT = 8; // a dot and its box
const CAP = 16; // the hint's key cap

type Box = { left: number; right: number; row: number };
type Layout = {
  /** The line's own box width, which the hint's share leaves it, or the whole box when wrapped. */
  line: number;
  /** The width its parts need on one row: the line's scrollWidth. Wrapped, the widest row. */
  content: number;
  /** Each part's box in the line, in order: the time, the plan, the tool segment. */
  parts: Box[];
  /** The hint's box in the status box, after the line, or on a row of its own when wrapped. */
  hint: Box | null;
};

const textWidth = (el: Element | null) => (el?.textContent ?? "").length * CH;

/** The status line laid out as composer.css draws it at each fit: the line's parts in a row, parted by the gap, the
 * hint at the right with its dot, its words, its "Turn to it" (gone tight or wrapped) and its key cap. Wrapped, the
 * parts flow onto as many rows of the box's width as they need, and the hint takes its own row. */
function layout(status: HTMLElement): Layout {
  const W = boxWidth ?? 0;
  const fit = status.dataset["fit"] ?? "full";
  const words = (el: Element | null) =>
    textWidth(el) + (el?.querySelector(".room-dot") ? DOT + GAP : 0);
  const partEls = [
    ...status.querySelectorAll(".room-status-took, .room-status-plan, .room-status-tools"),
  ];
  const sizes = partEls.map(words);
  const hintEl = status.querySelector(".room-hint");
  const hintSize =
    hintEl === null
      ? 0
      : DOT +
        GAP +
        textWidth(hintEl.querySelector(".room-hint-what")) +
        GAP +
        DOT +
        (fit === "full" ? textWidth(hintEl.querySelector(".room-hint-turn")) + GAP : 0) +
        CAP;
  const oneRow = sizes.reduce((a, b) => a + b, 0) + GAP * Math.max(0, sizes.length - 1);
  if (fit !== "wrapped") {
    const line = Math.max(0, W - (hintEl === null ? 0 : hintSize + GAP));
    let x = 0;
    const parts = sizes.map((size) => {
      const box = { left: x, right: x + size, row: 0 };
      x += size + GAP;
      return box;
    });
    const hint = hintEl === null ? null : { left: W - hintSize, right: W, row: 0 };
    return { line, content: oneRow, parts, hint };
  }
  let x = 0;
  let row = 0;
  const parts = sizes.map((size) => {
    if (x > 0 && x + size > W) {
      row += 1;
      x = 0;
    }
    const box = { left: x, right: x + size, row };
    x += size + GAP;
    return box;
  });
  const content = Math.max(0, ...parts.map((b) => b.right));
  const hint = hintEl === null ? null : { left: 0, right: hintSize, row: row + 1 };
  return { line: W, content, parts, hint };
}

/** Nothing stands past the line's box or the status box, and no two boxes on a row overlap. */
function expectNoOverflow(status: HTMLElement) {
  const l = layout(status);
  const W = boxWidth ?? 0;
  for (const part of l.parts) expect(part.right).toBeLessThanOrEqual(l.line);
  if (l.hint !== null) {
    expect(l.hint.left).toBeGreaterThanOrEqual(0);
    expect(l.hint.right).toBeLessThanOrEqual(W);
  }
  const boxes = [...l.parts, ...(l.hint === null ? [] : [l.hint])];
  for (const a of boxes)
    for (const b of boxes)
      if (a !== b && a.row === b.row) expect(a.right <= b.left || b.right <= a.left).toBe(true);
}

const HINT = {
  what: "Draft reply ready",
  label: "Draft reply, ready and not yet opened",
  state: "ready" as const,
  onTurn: () => {},
};

const fullest = (
  <StatusLine
    took="Took 38 seconds"
    plan="Uses your Claude plan"
    hint={HINT}
    tools={segment("Web search and browser on")}
  />
);

test("the time, the plan, the tool line and the hint, narrower than they need: the hint drops its words first, and nothing stands past the line", async () => {
  // one row needs 536 px for the line and 272 for the hint in full; tight, the hint takes 184
  boxWidth = 760;
  await mount(fullest);
  expect(status()?.dataset["fit"]).toBe("tight");
  expect(host!.querySelector(".room-hint-what")?.textContent).toBe("Draft reply ready");
  expect(tools()?.textContent).toBe("Web search and browser on");
  const l = layout(status()!);
  expect(l.content).toBeLessThanOrEqual(l.line);
  expectNoOverflow(status()!);
});

test("narrower still, the line wraps to two, each part whole on its row, and nothing stands past the box", async () => {
  boxWidth = 400;
  await mount(fullest);
  expect(status()?.dataset["fit"]).toBe("wrapped");
  const l = layout(status()!);
  expect(l.content).toBeLessThanOrEqual(l.line);
  expect(Math.max(...l.parts.map((p) => p.row))).toBe(1);
  expectNoOverflow(status()!);
});

test("with room for everything the line stays full, the hint keeping its words", async () => {
  boxWidth = 900;
  await mount(fullest);
  expect(status()?.dataset["fit"]).toBe("full");
  expectNoOverflow(status()!);
});

test("wrapped onto a row of its own, the tool line drops its dot; beside the plan it keeps it", async () => {
  const rect = Object.getOwnPropertyDescriptor(proto, "getBoundingClientRect");
  /** Where the tool segment starts, against the line's left edge at 0. */
  let toolsLeft = 0;
  proto.getBoundingClientRect = function (this: HTMLElement) {
    const left = this.classList.contains("room-status-tools") ? toolsLeft : 0;
    const width = this.classList.contains("room-status-line") ? 346 : 0;
    return {
      left,
      right: left + width,
      width,
      top: 0,
      bottom: 0,
      height: 0,
      x: left,
      y: 0,
    } as DOMRect;
  };
  try {
    toolsLeft = 0;
    await mount(fullest);
    expect(tools()?.dataset["lead"]).toBe("");
    await act(async () => root?.unmount());
    toolsLeft = 240;
    root = createRoot(host!);
    await act(async () => root?.render(wrap(fullest)));
    expect(tools()?.dataset["lead"]).toBeUndefined();
  } finally {
    if (rect !== undefined) Object.defineProperty(proto, "getBoundingClientRect", rect);
    else delete (proto as { getBoundingClientRect?: unknown }).getBoundingClientRect;
  }
});
