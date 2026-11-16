// @vitest-environment happy-dom
// The pen asks for frames only while it writes, the page is shown and it is in view; under reduced motion and in lite
// it draws one still frame and asks for none. axe: ui/app/src/room/pen-axe.test.tsx.
import { act, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { DEFAULT_DISPLAY, showDisplay, type Display } from "../lib/display.ts";
import { inkPoints } from "./ink-lookup.ts";
import { PenMark } from "./pen-mark.tsx";
import { RoomScopeProvider } from "./room-scope-provider.tsx";
import { WorkingLine, WorkingTime } from "./working-line.tsx";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

// happy-dom has no SVG geometry: a straight path of 1000 units stands in
const proto = SVGElement.prototype as unknown as Record<string, unknown>;
proto["getTotalLength"] = () => 1000;
proto["getPointAtLength"] = (k: number) => ({ x: k, y: 0 });

let frames: Map<number, FrameRequestCallback>;
let next = 1;
let observers: { cb: IntersectionObserverCallback; el: Element | null; on: boolean }[] = [];
let hidden = false;

beforeEach(() => {
  frames = new Map();
  observers = [];
  hidden = false;
  vi.stubGlobal("requestAnimationFrame", (cb: FrameRequestCallback) => {
    const id = next++;
    frames.set(id, cb);
    return id;
  });
  vi.stubGlobal("cancelAnimationFrame", (id: number) => frames.delete(id));
  vi.stubGlobal(
    "IntersectionObserver",
    class {
      o: { cb: IntersectionObserverCallback; el: Element | null; on: boolean };
      constructor(cb: IntersectionObserverCallback) {
        this.o = { cb, el: null, on: true };
        observers.push(this.o);
      }
      observe(el: Element) {
        this.o.el = el;
      }
      disconnect() {
        this.o.on = false;
      }
    },
  );
  Object.defineProperty(document, "hidden", { configurable: true, get: () => hidden });
});

let host: HTMLDivElement | null = null;
let root: ReturnType<typeof createRoot> | null = null;

async function mount(node: ReactNode, display: Partial<Display> = {}) {
  showDisplay({ ...DEFAULT_DISPLAY, ...display });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root?.render(<RoomScopeProvider>{node}</RoomScopeProvider>));
}

afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  root = null;
  showDisplay(DEFAULT_DISPLAY);
  vi.unstubAllGlobals();
});

/** Runs the frames asked for, at a time. */
const tick = (now: number) => {
  const due = [...frames.values()];
  frames.clear();
  for (const cb of due) cb(now);
};
const asking = () => frames.size > 0;
const ink = () => document.querySelector(".pen-mark path")?.getAttribute("stroke-dasharray");
const setHidden = (h: boolean) =>
  act(async () => {
    hidden = h;
    document.dispatchEvent(new Event("visibilitychange"));
  });
const setInView = (on: boolean) =>
  act(async () => {
    for (const o of observers.filter((x) => x.on)) {
      o.cb(
        [{ isIntersecting: on, target: o.el } as IntersectionObserverEntry],
        {} as IntersectionObserver,
      );
    }
  });

test("it writes: each frame moves the ink on, at most 30 a second", async () => {
  await mount(<PenMark />);
  expect(asking()).toBe(true);
  const first = ink();
  tick(performance.now() + 40);
  const second = ink();
  expect(second).not.toBe(first);
  // 10 ms on: under a 30th of a second, so it asks again without drawing
  tick(performance.now() + 50);
  expect(ink()).toBe(second);
  expect(asking()).toBe(true);
});

test("hidden, it stops asking for frames, and shown again it resumes", async () => {
  await mount(<PenMark />);
  expect(asking()).toBe(true);
  await setHidden(true);
  expect(asking()).toBe(false);
  await setHidden(false);
  expect(asking()).toBe(true);
});

test("off-screen it stops, and back in view it resumes", async () => {
  await mount(<PenMark />);
  await setInView(false);
  expect(asking()).toBe(false);
  await setInView(true);
  expect(asking()).toBe(true);
});

test("done, it stops: when no longer writing, and when it goes", async () => {
  await mount(<PenMark />);
  await act(async () =>
    root?.render(
      <RoomScopeProvider>
        <PenMark writing={false} />
      </RoomScopeProvider>,
    ),
  );
  expect(asking()).toBe(false);
  expect(ink()).not.toBe("0 99999");
  await act(async () =>
    root?.render(
      <RoomScopeProvider>
        <PenMark />
      </RoomScopeProvider>,
    ),
  );
  expect(asking()).toBe(true);
  await act(async () => root?.render(<RoomScopeProvider>{null}</RoomScopeProvider>));
  expect(asking()).toBe(false);
  expect(observers.every((o) => !o.on)).toBe(true);
});

test("under reduced motion and in lite it draws one still frame and asks for none", async () => {
  await mount(<PenMark />, { motion: "reduced" });
  expect(asking()).toBe(false);
  expect(ink()).not.toBe("0 99999");
  expect(document.querySelector(".pen-mark")?.hasAttribute("data-still")).toBe(true);
  await act(async () => showDisplay({ ...DEFAULT_DISPLAY, render: "lite" }));
  expect(asking()).toBe(false);
  await act(async () => showDisplay(DEFAULT_DISPLAY));
  expect(asking()).toBe(true);
});

test("the working line writes with the pen only while working, and keeps its words", async () => {
  await mount(
    <WorkingLine state={{ kind: "wait", words: "Answering.", pill: null, working: true }} />,
  );
  expect(document.querySelector(".working .pen-mark")).not.toBeNull();
  expect(document.querySelector(".working")?.textContent).toContain("Answering.");
  await act(async () =>
    root?.render(
      <RoomScopeProvider>
        <WorkingLine state={{ kind: "series", total: 4, current: null, expected: "16:32" }} />
      </RoomScopeProvider>,
    ),
  );
  expect(document.querySelector(".working .pen-mark")).toBeNull();
  expect(asking()).toBe(false);
});

test("a check and a series with no expected time have no pill, in the row or beside Stop", async () => {
  await mount(
    <>
      <WorkingLine
        state={{
          kind: "check",
          name: "Acme Ltd v. State",
          waitingFor: "the judgments",
          expected: null,
        }}
      />
      <WorkingTime
        state={{
          kind: "check",
          name: "Acme Ltd v. State",
          waitingFor: "the judgments",
          expected: null,
        }}
      />
      <WorkingLine
        state={{
          kind: "series",
          total: 4,
          current: "Pooja Ravinder Devidasani",
          expected: null,
          working: true,
        }}
      />
      <WorkingTime state={{ kind: "series", total: 4, current: null, expected: null }} />
    </>,
  );
  expect(document.querySelectorAll(".working")).toHaveLength(2);
  expect(document.querySelector(".working-time")).toBeNull();
  expect(document.querySelectorAll(".working")[1]?.textContent).toContain(
    "Pooja Ravinder Devidasani",
  );
  await act(async () =>
    root?.render(
      <RoomScopeProvider>
        <WorkingLine state={{ kind: "series", total: 4, current: null, expected: "16:32" }} />
      </RoomScopeProvider>,
    ),
  );
  expect(document.querySelector(".working-time")?.textContent).toMatch(/Expected by/);
});

test("the page's geometry is never walked: no getPointAtLength at setup, in a frame, or for a second pen", async () => {
  // Blink measures the whole path again on every read (0.9 ms each for the pen's ink in Chrome): the 2,380 reads of the
  // old table held the first run's "Setting up" off the screen for about 8 s on the Windows VM (webview-stall).
  // happy-dom puts the geometry on SVGGeometryElement, so the stubs above do not reach a path: stub the path's own
  const path = SVGPathElement.prototype as unknown as Record<string, unknown>;
  const walk = vi.fn((k: number) => ({ x: k, y: 0 }));
  const total = vi.fn(() => 1000);
  path["getPointAtLength"] = walk;
  path["getTotalLength"] = total;
  try {
    await mount(<PenMark />);
    const nib = () => document.querySelector(".pen-mark > g > g")?.getAttribute("transform");
    const start = nib();
    for (let i = 1; i <= 20; i++) tick(performance.now() + 40 * i);
    expect(nib()).not.toBe(start);
    await act(async () => root?.render(<RoomScopeProvider>{null}</RoomScopeProvider>));
    await act(async () =>
      root?.render(
        <RoomScopeProvider>
          <PenMark />
        </RoomScopeProvider>,
      ),
    );
    expect(walk).not.toHaveBeenCalled();
    // one length a pen, for the dash
    expect(total).toHaveBeenCalledTimes(2);
  } finally {
    delete path["getPointAtLength"];
    delete path["getTotalLength"];
  }
});

test("the ink's points follow the path's length, and clamp outside it", () => {
  // a straight cubic whose controls bunch toward its start: by length, not by the curve's own parameter
  const straight = inkPoints("M 0 0 c 1 0 2 0 30 0");
  expect(straight.length).toBeCloseTo(30, 9);
  expect(straight.at(12.5).x).toBeCloseTo(12.5, 9);
  expect(straight.at(12.5).y).toBe(0);
  expect(straight.at(-5)).toEqual({ x: 0, y: 0 });
  expect(straight.at(1e6)).toEqual({ x: 30, y: 0 });

  // two straight cubics around a corner, the second relative to the first's end
  const corner = inkPoints("M 5 5 c 0 0 10 0 10 0 c 0 0 0 10 0 10");
  expect(corner.length).toBeCloseTo(20, 9);
  expect(corner.at(15).x).toBeCloseTo(15, 9);
  expect(corner.at(15).y).toBeCloseTo(10, 9);

  // a quarter of a circle by its usual cubic: the length within 0.1% of pi/2, a point halfway on the circle
  const r = 100;
  const h = 0.5523 * r;
  const arc = inkPoints(`M ${r} 0 c 0 ${h} ${h - r} ${r} ${-r} ${r}`);
  expect(Math.abs(arc.length / ((Math.PI * r) / 2) - 1)).toBeLessThan(0.001);
  const mid = arc.at(arc.length / 2);
  expect(Math.hypot(mid.x, mid.y)).toBeCloseTo(r, 0);
  expect(mid.x).toBeCloseTo(mid.y, 0);

  expect(() => inkPoints("M 0 0 l 10 10")).toThrow(/only relative cubics/);
  expect(() => inkPoints("c 1 2 3 4 5 6")).toThrow(/start with M/);
});
