// @vitest-environment happy-dom
// A text over UNDOABLE_MAX, such as a long question coming back after its turn stopped, is set in place, which the
// engine's undo can't take back: Cmd/Ctrl+Z puts the words before it back all the same (#978).
import { afterEach, expect, test } from "vitest";
import { replaceAll, UNDOABLE_MAX } from "./composer-edit.ts";

afterEach(() => {
  document.body.replaceChildren();
});

const long = "Clause 00001. The parties agree.\n".repeat(Math.ceil((UNDOABLE_MAX + 1) / 33));

function field(value: string): HTMLTextAreaElement {
  const a = document.createElement("textarea");
  a.value = value;
  document.body.append(a);
  a.focus();
  return a;
}

/** Presses `key` on the field; whether something took it (its default prevented). */
function press(a: HTMLTextAreaElement, init: KeyboardEventInit): boolean {
  const e = new KeyboardEvent("keydown", { key: "z", bubbles: true, cancelable: true, ...init });
  a.dispatchEvent(e);
  return e.defaultPrevented;
}

test("Cmd+Z after a long text set in place puts the words before back, as an undo the field hears", () => {
  const a = field("Draft of the notice");
  const heard: string[] = [];
  a.addEventListener("input", (e) => heard.push((e as InputEvent).inputType));
  expect(long.length).toBeGreaterThan(UNDOABLE_MAX);
  expect(replaceAll(a, long, true)).toBe(false);
  expect(a.value).toBe(long);
  expect(press(a, { metaKey: true })).toBe(true);
  expect(a.value).toBe("Draft of the notice");
  expect(heard).toEqual(["historyUndo"]);
  // one step: a second press is the engine's
  expect(press(a, { metaKey: true })).toBe(false);
  expect(a.value).toBe("Draft of the notice");
});

test("Ctrl+Z takes it back too; Shift, Alt or both modifiers leave the key alone", () => {
  const a = field("");
  replaceAll(a, long, true);
  expect(press(a, { metaKey: true, shiftKey: true })).toBe(false);
  expect(press(a, { ctrlKey: true, altKey: true })).toBe(false);
  expect(press(a, { metaKey: true, ctrlKey: true })).toBe(false);
  expect(press(a, { key: "y", ctrlKey: true })).toBe(false);
  expect(a.value).toBe(long);
  expect(press(a, { ctrlKey: true })).toBe(true);
  expect(a.value).toBe("");
});

test("once the words changed after it, or another replaceAll came, the step is forgotten", () => {
  const a = field("Draft");
  replaceAll(a, long, true);
  a.value = `${long}more`;
  expect(press(a, { metaKey: true })).toBe(false);
  expect(a.value).toBe(`${long}more`);
  replaceAll(a, long, true);
  replaceAll(a, "Short", true);
  expect(press(a, { metaKey: true })).toBe(false);
  expect(a.value).toBe("Short");
});

test("a text at UNDOABLE_MAX set in place (no engine) gets no step of its own", () => {
  const a = field("Draft");
  replaceAll(a, long.slice(0, UNDOABLE_MAX), true);
  expect(press(a, { metaKey: true })).toBe(false);
  expect(a.value).toBe(long.slice(0, UNDOABLE_MAX));
});
