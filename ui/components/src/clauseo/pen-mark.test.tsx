// @vitest-environment happy-dom
// The pen asks for frames only while it writes, the page is shown and it is in view; under reduced motion and in lite
// it draws one still frame and asks for none. axe: ui/app/src/room/pen-axe.test.tsx.
import { act, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { DEFAULT_DISPLAY, showDisplay, type Display } from "../lib/display.ts";
import { inkLookup } from "./ink-lookup.ts";
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

test("each frame reads the nib's place from the table: the path is walked at setup only", async () => {
  // happy-dom puts the geometry on SVGGeometryElement, so the stubs above do not reach a path: stub the path's own
  const path = SVGPathElement.prototype as unknown as Record<string, unknown>;
  const walk = vi.fn((k: number) => ({ x: k, y: 0 }));
  path["getPointAtLength"] = walk;
  path["getTotalLength"] = () => 1000;
  try {
    await mount(<PenMark />);
    const nib = () => document.querySelector(".pen-mark > g > g")?.getAttribute("transform");
    const setup = walk.mock.calls.length;
    expect(setup).toBeGreaterThan(0);
    const start = nib();
    for (let i = 1; i <= 20; i++) tick(performance.now() + 40 * i);
    expect(nib()).not.toBe(start);
    expect(walk.mock.calls.length).toBe(setup);
  } finally {
    delete path["getPointAtLength"];
    delete path["getTotalLength"];
  }
});

test("the table reads the ink between its ends, and clamps outside them", () => {
  const straight = inkLookup((k) => ({ x: k, y: 2 * k }), 330, 540);
  expect(straight(400)).toEqual({ x: 400, y: 800 });
  expect(straight(401.1).x).toBeCloseTo(401.1, 9);
  expect(straight(330)).toEqual({ x: 330, y: 660 });
  expect(straight(540)).toEqual({ x: 540, y: 1080 });
  expect(straight(-5000)).toEqual(straight(330));
  expect(straight(1e6)).toEqual(straight(540));

  const curved = inkLookup((k) => ({ x: k, y: Math.sin(k) }), 330, 540);
  expect(curved(400.1).y).toBeCloseTo(Math.sin(400.1), 1);
});
