// @vitest-environment happy-dom
// The room keeps to the thread's end while a long answer streams in (thread.tsx over follow.ts), the transcript's
// scroller laid out by hand: its content as tall as the words it holds. Started at the end, it stays there through 500
// chunks and 20 tool calls, with Latest hidden; a wheel up mid-answer leaves the view where it is and shows Latest,
// and a question sent from the composer goes back to the end.
import { act } from "react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { header, reset, scriptedTurn, userMessage } from "../../testing/stream-script.ts";
import { mountRoom, type Harness } from "./harness.ts";

let h: Harness | null = null;
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
  await h?.unmount();
  h = null;
  document.body.replaceChildren();
  vi.unstubAllGlobals();
});

const VIEW = 600;

/** Lays the transcript's scroller out: a px of height for every two characters it holds, in a VIEW px view. */
function layOut(el: HTMLElement) {
  let y = 0;
  const height = () => Math.max(VIEW, 200 + Math.ceil((el.textContent ?? "").length / 2));
  Object.defineProperty(el, "scrollHeight", { configurable: true, get: height });
  Object.defineProperty(el, "clientHeight", { configurable: true, get: () => VIEW });
  Object.defineProperty(el, "scrollTop", {
    configurable: true,
    get: () => y,
    set: (v: number) => {
      y = Math.max(0, Math.min(v, height() - VIEW));
    },
  });
  return { gap: () => height() - y - VIEW, top: () => y, height };
}

/** After a frame's commit: layout reports sizes, then the scroll the follow set comes back as an event. */
function frame(el: HTMLElement) {
  act(() => {
    for (const cb of observers) cb([], {} as ResizeObserver);
    el.dispatchEvent(new Event("scroll"));
    for (const f of frames.splice(0)) f(0);
  });
}

const latest = () => h!.host.querySelector<HTMLElement>(".room-latest");

test("a long answer streamed in 500 chunks keeps the view at the thread's end, started there", async () => {
  h = await mountRoom();
  await h.deliverAwaited([
    reset([userMessage(5, 1, "What does clause nine say?")], header("running")),
  ]);
  await h.settle();
  const el = h.host.querySelector<HTMLElement>(".room-thread");
  if (el === null) throw new Error("no thread");
  const box = layOut(el);
  frame(el);
  expect(box.gap()).toBe(0);

  const { bodies, chunks } = scriptedTurn();
  expect(chunks).toBe(500);
  let grown = 0;
  for (let i = 0; i < bodies.length; i += 3) {
    const before = box.height();
    h.deliver(bodies.slice(i, i + 3));
    grown += box.height() > before ? 1 : 0;
    frame(el);
    expect(box.gap()).toBe(0);
  }
  // the answer outgrew the view many times over, and the view went with it
  expect(box.height()).toBeGreaterThan(VIEW * 5);
  expect(grown).toBeGreaterThan(100);
  expect(box.top()).toBe(box.height() - VIEW);
  expect(latest()?.hidden).toBe(true);
}, 30_000);

test("a wheel up mid-answer leaves the view where it is and shows Latest; a question sent goes back to the end", async () => {
  h = await mountRoom();
  await h.deliverAwaited([
    reset([userMessage(5, 1, "What does clause nine say?")], header("running")),
  ]);
  await h.settle();
  const el = h.host.querySelector<HTMLElement>(".room-thread")!;
  const box = layOut(el);
  frame(el);
  const { bodies } = scriptedTurn();
  const half = Math.floor(bodies.length / 2);
  for (let i = 0; i < half; i += 3) {
    h.deliver(bodies.slice(i, i + 3));
    frame(el);
  }
  act(() => {
    el.querySelector("[data-entry-id]")!.dispatchEvent(
      new WheelEvent("wheel", { deltaY: -60, bubbles: true }),
    );
  });
  el.scrollTop = box.top() - 500;
  frame(el);
  const y = box.top();
  for (let i = half; i < bodies.length; i += 3) {
    h.deliver(bodies.slice(i, i + 3));
    frame(el);
  }
  expect(box.top()).toBe(y);
  expect(box.gap()).toBeGreaterThan(1000);
  expect(latest()?.hidden).toBe(false);
  expect(latest()?.querySelector("button")?.getAttribute("aria-label")).toBe(
    "Go to the latest message",
  );

  // the answer ends; the lawyer asks again from the composer, and the view goes to the end with it
  h.deliver([{ upsert: { entry: header("idle") } }]);
  frame(el);
  await h.type("And clause ten?");
  await h.key({ key: "Enter" });
  await h.settle();
  frame(el);
  expect(h.sent("send_prompt")).toHaveLength(1);
  expect(box.gap()).toBe(0);
  expect(latest()?.hidden).toBe(true);
}, 30_000);

test("Latest goes to the end and follows the answer again", async () => {
  h = await mountRoom();
  await h.deliverAwaited([
    reset([userMessage(5, 1, "What does clause nine say?")], header("running")),
  ]);
  await h.settle();
  const el = h.host.querySelector<HTMLElement>(".room-thread")!;
  const box = layOut(el);
  frame(el);
  const { bodies } = scriptedTurn();
  for (let i = 0; i < 120; i += 3) {
    h.deliver(bodies.slice(i, i + 3));
    frame(el);
  }
  act(() => {
    el.dispatchEvent(new KeyboardEvent("keydown", { key: "PageUp", bubbles: true }));
  });
  el.scrollTop = 0;
  frame(el);
  expect(latest()?.hidden).toBe(false);
  await h.click(latest()?.querySelector("button"));
  frame(el);
  expect(latest()?.hidden).toBe(true);
  for (let i = 120; i < 300; i += 3) {
    h.deliver(bodies.slice(i, i + 3));
    frame(el);
    expect(box.gap()).toBe(0);
  }
});
