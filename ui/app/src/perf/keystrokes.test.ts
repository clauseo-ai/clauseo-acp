// @vitest-environment happy-dom
import { describe, expect, test } from "vitest";
import { KeystrokeRecorder, type EventEntry, type EventTimingSource } from "./keystrokes.ts";

function rig(entries: (composer: Element) => EventEntry[], supported = true) {
  document.body.innerHTML = '<textarea data-testid="composer"></textarea><input id="other">';
  const composer = document.querySelector("textarea")!;
  let clock = 0;
  const timing: EventTimingSource = { start: () => {}, take: () => entries(composer) };
  const r = new KeystrokeRecorder({
    timing: supported ? timing : null,
    composer: () => document.querySelector('[data-testid="composer"]'),
    events: window,
    now: () => clock,
  });
  /** Dispatches `type` at `target`, its handlers taking `ms` on the page clock; returns its timeStamp. */
  const fire = (target: Element, type: string, ms: number): number => {
    const e = new Event(type, { bubbles: true });
    const handler = () => (clock += ms);
    target.addEventListener(type, handler);
    target.dispatchEvent(e);
    target.removeEventListener(type, handler);
    return e.timeStamp;
  };
  return { r, composer, fire };
}

const entry = (
  name: string,
  startTime: number,
  duration: number,
  processing: number,
  target: Node,
) => ({
  name,
  startTime,
  duration,
  processingStart: startTime + 1,
  processingEnd: startTime + 1 + processing,
  target,
});

describe("KeystrokeRecorder", () => {
  test("one sample per composer keydown: entries when reported, else 16 and the page's own bracket", () => {
    const stamps: number[] = [];
    const { r, composer, fire } = rig((c) => [
      // keystroke 1: keydown and input reported; the longest duration and the summed processing count
      entry("keydown", stamps[0]!, 24, 2, c),
      entry("input", stamps[1]!, 20, 3, c),
      entry("keyup", stamps[1]! + 30, 40, 9, c), // not part of a keystroke
      entry("keydown", stamps[0]!, 99, 99, document.querySelector("#other")!), // another element
    ]);
    r.start();
    stamps.push(fire(composer, "keydown", 1));
    stamps.push(fire(composer, "input", 2));
    fire(composer, "keydown", 0.5); // keystroke 2: no entry
    fire(composer, "input", 0.25);
    fire(document.querySelector("#other")!, "keydown", 5); // not the composer
    const sample = r.stop();
    expect(sample.latency_ms).toEqual([24, 16]);
    expect(sample.handler_ms).toEqual([5, 0.75]);
    expect(sample.entry_share).toBe(0.5);
  });

  test("listeners come off at stop", () => {
    const { r, composer, fire } = rig(() => []);
    r.start();
    fire(composer, "keydown", 1);
    expect(r.stop().latency_ms).toEqual([16]);
    fire(composer, "keydown", 1);
    r.start();
    expect(r.stop().latency_ms).toEqual([]);
  });

  test("without Event Timing (WebKit) every list is null", () => {
    const { r, composer, fire } = rig(() => [], false);
    r.start();
    fire(composer, "keydown", 1);
    expect(r.stop()).toEqual({ latency_ms: null, handler_ms: null, entry_share: null });
  });
});
