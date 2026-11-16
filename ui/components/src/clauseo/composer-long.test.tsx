// @vitest-environment happy-dom
// The composer with a long text in it, such as a pasted brief of megabytes: a field over its six-line cap is fitted
// with one read per edit, and text too long for the engine's line-at-a-time insertText is set in place.
import { act, createRef, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, test, vi } from "vitest";
import { TooltipProvider } from "../ui/tooltip.tsx";
import { DEFAULT_DISPLAY, showDisplay } from "../lib/display.ts";
import { Composer, type ComposerHandle, type ComposerProps } from "./composer.tsx";
import { UNDOABLE_MAX } from "./composer-edit.ts";
import { RoomScopeProvider } from "./room-scope-provider.tsx";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
if (!("fonts" in document)) {
  Object.defineProperty(document, "fonts", {
    value: { ready: Promise.resolve(), status: "loaded", load: async () => [] },
  });
}

let host: HTMLDivElement | null = null;
let root: ReturnType<typeof createRoot> | null = null;

async function mount(node: ReactNode) {
  showDisplay({ ...DEFAULT_DISPLAY, motion: "reduced" });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () =>
    root?.render(
      <TooltipProvider>
        <RoomScopeProvider>{node}</RoomScopeProvider>
      </TooltipProvider>,
    ),
  );
}

afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  host = null;
  root = null;
  showDisplay(DEFAULT_DISPLAY);
  Reflect.deleteProperty(document, "execCommand");
  vi.useRealTimers();
});

const composer = (p: Partial<ComposerProps>) => (
  <Composer took={null} plan="" hint={null} testIds={{ field: "composer", ask: "send" }} {...p} />
);
const field = () => document.querySelector<HTMLTextAreaElement>('[data-testid="composer"]')!;
const type = (text: string) =>
  act(async () => {
    Reflect.set(HTMLTextAreaElement.prototype, "value", text, field());
    field().dispatchEvent(new Event("input", { bubbles: true }));
  });

/** The engine's editing, as WebKit and Chromium do it: the selection replaced, then an input event. */
function engine() {
  const calls: [string, string | undefined][] = [];
  const exec = (command: string, _ui?: boolean, value?: string) => {
    calls.push([command, value]);
    const a = document.activeElement;
    if (!(a instanceof HTMLTextAreaElement)) return false;
    const insert = command === "delete" ? "" : (value ?? "");
    const next = a.value.slice(0, a.selectionStart) + insert + a.value.slice(a.selectionEnd);
    Reflect.set(HTMLTextAreaElement.prototype, "value", next, a);
    a.dispatchEvent(new Event("input", { bubbles: true }));
    return true;
  };
  Object.defineProperty(document, "execCommand", { value: exec, configurable: true });
  return calls;
}

/** A field that wraps `perLine` characters to a `line` px line, as composer-field.test.tsx's: its client height is
 * its set height or one line, its scroll height its words' lines when they need more. */
function wraps(a: HTMLTextAreaElement, line = 26, perLine = 40) {
  const client = () => (a.style.height === "" ? line : parseFloat(a.style.height));
  Object.defineProperty(a, "clientHeight", { configurable: true, get: client });
  Object.defineProperty(a, "scrollHeight", {
    configurable: true,
    get: () => Math.max(client(), Math.ceil(a.value.length / perLine) * line),
  });
}

test("over the cap, an edit that leaves it over reads only the scroll height: no style write, no fit once typing pauses", async () => {
  await mount(composer({}));
  const a = field();
  wraps(a);
  const long = "The parties agree that notice is given in writing. ".repeat(40);
  await type(long.slice(0, 1));
  // a paste of far more than six lines: the field stops at the cap and scrolls
  await type(long);
  expect(a.style.height).toBe("156px");
  expect(a.style.overflowY).toBe("");
  vi.useFakeTimers({ toFake: ["setTimeout"] });
  const writes: string[] = [];
  const seen = new MutationObserver((records) => {
    for (const _ of records) writes.push(a.style.cssText);
  });
  seen.observe(a, { attributes: true, attributeFilter: ["style"] });
  let reads = 0;
  const scroll = Object.getOwnPropertyDescriptor(a, "scrollHeight")!;
  Object.defineProperty(a, "scrollHeight", {
    configurable: true,
    get: () => {
      reads += 1;
      return scroll.get!.call(a);
    },
  });
  // typed on, then Backspace, then a word cut from the middle: each still well over the cap
  await type(`${long}x`);
  await type(long);
  await type(long.slice(0, 500) + long.slice(600));
  await act(async () => vi.advanceTimersByTime(1000));
  seen.disconnect();
  vi.useRealTimers();
  // one read for each edit, and the pause fits nothing: a whole fit would have collapsed the field to measure it
  expect(reads).toBe(3);
  expect(writes).toEqual([]);
  expect(a.style.height).toBe("156px");
  // cut back under the cap, it is fitted whole again and shrinks
  await type(long.slice(0, 50));
  expect(a.style.height).toBe("52px");
  expect(a.style.overflowY).toBe("hidden");
});

test("setText over UNDOABLE_MAX sets the words in place, never through the engine's line-at-a-time insertText", async () => {
  const calls = engine();
  const onInput = vi.fn();
  const ref = createRef<ComposerHandle>();
  await mount(composer({ ref, onInput }));
  const big = "Clause 00001. The parties agree.\n"
    .repeat(Math.ceil((UNDOABLE_MAX + 1) / 33))
    .slice(0, UNDOABLE_MAX + 1);
  await act(async () => ref.current?.setText(big));
  expect(calls).toEqual([]);
  expect(field().value).toBe(big);
  expect(document.activeElement).toBe(field());
  expect(onInput).toHaveBeenCalledExactlyOnceWith(big);
  // at the limit it still goes through the engine, so undo takes it back
  const most = big.slice(0, UNDOABLE_MAX);
  await act(async () => ref.current?.setText(most));
  expect(calls).toEqual([["insertText", most]]);
  expect(field().value).toBe(most);
});
