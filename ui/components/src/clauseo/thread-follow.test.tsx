// @vitest-environment happy-dom
// The thread follows its end in the room's own scroller (thread.tsx over follow.ts): growth keeps the view at the end;
// a wheel up lets go and Latest shows, outside the log, with its name; Latest goes back, gliding only where motion is
// allowed; a question sent goes back; Find's hold keeps it.
import { act, StrictMode, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { DEFAULT_DISPLAY, showDisplay } from "../lib/display.ts";
import { RoomScopeProvider } from "./room-scope-provider.tsx";
import { Thread } from "./thread.tsx";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
if (!("fonts" in document)) {
  Object.defineProperty(document, "fonts", {
    value: { ready: Promise.resolve(), status: "loaded", load: async () => [] },
  });
}

let host: HTMLDivElement | null = null;
let root: ReturnType<typeof createRoot> | null = null;
let observers: ResizeObserverCallback[] = [];
let frames: FrameRequestCallback[] = [];

beforeEach(() => {
  observers = [];
  frames = [];
  vi.stubGlobal(
    "ResizeObserver",
    class {
      constructor(cb: ResizeObserverCallback) {
        observers.push(cb);
      }
      observe() {}
      disconnect() {}
    },
  );
  vi.stubGlobal("requestAnimationFrame", (cb: FrameRequestCallback) => frames.push(cb));
  vi.stubGlobal("cancelAnimationFrame", () => {});
});

afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  root = null;
  showDisplay(DEFAULT_DISPLAY);
  vi.unstubAllGlobals();
});

/** The thread's scroller, laid out: `height` px of content in a 500 px view, its scroll clamped as a browser's is. */
type Box = { height: number; y: number; smooth: number };

function layOut(el: HTMLElement, box: Box) {
  Object.defineProperty(el, "scrollHeight", { configurable: true, get: () => box.height });
  Object.defineProperty(el, "clientHeight", { configurable: true, get: () => 500 });
  Object.defineProperty(el, "clientWidth", { configurable: true, get: () => 700 });
  Object.defineProperty(el, "scrollTop", {
    configurable: true,
    get: () => box.y,
    set: (y: number) => {
      box.y = Math.max(0, Math.min(y, box.height - 500));
    },
  });
  el.scrollTo = ((o: ScrollToOptions) => {
    if (o.behavior === "smooth") box.smooth += 1;
    el.scrollTop = o.top ?? 0;
  }) as typeof el.scrollTo;
}

function Room({ children, sent, hold }: { children?: ReactNode; sent?: string; hold?: boolean }) {
  return (
    <Thread question="What did the bank know?" who="You" sent={sent ?? null} hold={hold ?? false}>
      {children ?? <p>The bank had notice.</p>}
    </Thread>
  );
}

async function mount(node: ReactNode, box: Box, motion: "reduced" | "full" = "reduced") {
  showDisplay({ ...DEFAULT_DISPLAY, motion });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  // the scroller is laid out before the observer's first report (laidOut)
  await act(async () =>
    root?.render(
      <StrictMode>
        <RoomScopeProvider>{node}</RoomScopeProvider>
      </StrictMode>,
    ),
  );
  const t = host.querySelector<HTMLElement>(".room-thread")!;
  layOut(t, box);
  return t;
}

async function rerender(node: ReactNode) {
  await act(async () =>
    root?.render(
      <StrictMode>
        <RoomScopeProvider>{node}</RoomScopeProvider>
      </StrictMode>,
    ),
  );
}

/** Layout ran: every observer reports, as after a frame's layout. */
const laidOut = () => act(() => observers.forEach((cb) => cb([], {} as ResizeObserver)));
/** A scroll event, and the frame its handler waits for. */
async function scrolled(t: HTMLElement) {
  await act(async () => {
    t.dispatchEvent(new Event("scroll"));
    frames.splice(0).forEach((f) => f(0));
  });
}
const latest = () => host!.querySelector<HTMLElement>(".room-latest")!;
const atEnd = (b: Box) => b.y === b.height - 500;

test("growth keeps the view at the end; a wheel up lets go and Latest shows, outside the log, named", async () => {
  const box: Box = { height: 2000, y: 0, smooth: 0 };
  const t = await mount(<Room />, box);
  laidOut();
  expect(atEnd(box)).toBe(true);
  for (let i = 0; i < 20; i += 1) {
    box.height += 30;
    laidOut();
    await scrolled(t);
    expect(atEnd(box)).toBe(true);
  }
  expect(latest().hidden).toBe(true);

  await act(async () => {
    t.querySelector("p")!.dispatchEvent(new WheelEvent("wheel", { deltaY: -40, bubbles: true }));
  });
  box.y -= 400;
  await scrolled(t);
  const y = box.y;
  box.height += 300;
  laidOut();
  expect(box.y).toBe(y);
  expect(latest().hidden).toBe(false);
  expect(t.contains(latest())).toBe(false);
  expect(latest().closest("[role=log]")).toBeNull();
  const button = latest().querySelector("button")!;
  expect(button.getAttribute("aria-label")).toBe("Go to the latest message");
  expect(button.textContent).toBe("Latest");
  expect(button.querySelector("svg")?.getAttribute("aria-hidden")).toBe("true");
});

test("a wheel down, or a wheel over something else than the thread, lets nothing go", async () => {
  const box: Box = { height: 2000, y: 0, smooth: 0 };
  const t = await mount(<Room />, box);
  laidOut();
  await act(async () => {
    t.dispatchEvent(new WheelEvent("wheel", { deltaY: 40, bubbles: true }));
    document.body.dispatchEvent(new WheelEvent("wheel", { deltaY: -40, bubbles: true }));
  });
  box.height += 200;
  laidOut();
  expect(atEnd(box)).toBe(true);
});

test("Latest goes to the end at once under reduced motion, hides, and follows again", async () => {
  const box: Box = { height: 2000, y: 0, smooth: 0 };
  const t = await mount(<Room />, box);
  laidOut();
  await act(async () =>
    t.dispatchEvent(new KeyboardEvent("keydown", { key: "PageUp", bubbles: true })),
  );
  box.y = 100;
  await scrolled(t);
  expect(latest().hidden).toBe(false);
  await act(async () => latest().querySelector("button")!.click());
  expect(atEnd(box)).toBe(true);
  expect(box.smooth).toBe(0);
  expect(latest().hidden).toBe(true);
  box.height += 120;
  laidOut();
  expect(atEnd(box)).toBe(true);
});

test("with motion allowed, Latest glides", async () => {
  const box: Box = { height: 2000, y: 0, smooth: 0 };
  const t = await mount(<Room />, box, "full");
  laidOut();
  await act(async () =>
    t.dispatchEvent(new KeyboardEvent("keydown", { key: "Home", bubbles: true })),
  );
  box.y = 0;
  await scrolled(t);
  await act(async () => latest().querySelector("button")!.click());
  expect(box.smooth).toBe(1);
  expect(atEnd(box)).toBe(true);
});

test("a question sent goes to the end and follows", async () => {
  const box: Box = { height: 2000, y: 0, smooth: 0 };
  const t = await mount(<Room />, box);
  laidOut();
  await act(async () => t.dispatchEvent(new WheelEvent("wheel", { deltaY: -40 })));
  box.y = 0;
  await scrolled(t);
  box.height += 200;
  laidOut();
  expect(box.y).toBe(0);
  await rerender(<Room sent="turn-2" />);
  expect(atEnd(box)).toBe(true);
  box.height += 50;
  laidOut();
  expect(atEnd(box)).toBe(true);
});

test("Find's jump holds the view where it landed, and the stream doesn't take it away", async () => {
  const box: Box = { height: 6000, y: 0, smooth: 0 };
  const t = await mount(<Room hold />, box);
  box.y = 1500;
  laidOut();
  expect(box.y).toBe(1500);
  await rerender(<Room />);
  await scrolled(t);
  box.height += 400;
  laidOut();
  expect(box.y).toBe(1500);
  expect(latest().hidden).toBe(false);
});
