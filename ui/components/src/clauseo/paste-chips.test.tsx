// @vitest-environment happy-dom
// A long paste on the composer: a chip at PASTE_CHIP_MIN characters and over, words in the field under it; the chips'
// texts after the typed words in what goes and what the room hears; removal; and the chips coming back with the
// composer's words through every way they come back (a draft, setText, fill, clear and its undo).
import { act, createRef, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { TooltipProvider } from "../ui/tooltip.tsx";
import { DEFAULT_DISPLAY, showDisplay } from "../lib/display.ts";
import { Composer, type ComposerHandle, type ComposerProps } from "./composer.tsx";
import { RoomScopeProvider } from "./room-scope-provider.tsx";
import {
  askedOf,
  chipOf,
  chipTail,
  forgetChipsForTests,
  PASTE_CHIP_MIN,
  splitAsked,
  wordCount,
} from "./paste-chips-logic.ts";

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

beforeEach(() => forgetChipsForTests());
afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  host = null;
  root = null;
  showDisplay(DEFAULT_DISPLAY);
});

const composer = (p: Partial<ComposerProps>) => (
  <Composer took={null} plan="" hint={null} testIds={{ field: "composer", ask: "send" }} {...p} />
);
const field = () => document.querySelector<HTMLTextAreaElement>('[data-testid="composer"]')!;
const chips = () => [...document.querySelectorAll<HTMLElement>('[data-testid="paste-chip"]')];
const type = (text: string) =>
  act(async () => {
    Reflect.set(HTMLTextAreaElement.prototype, "value", text, field());
    field().dispatchEvent(new Event("input", { bubbles: true }));
  });

/** A paste of `text` on the field, as the engine fires it; true when the composer took it (its default prevented). */
async function paste(text: string): Promise<boolean> {
  const e = new Event("paste", { bubbles: true, cancelable: true });
  Object.defineProperty(e, "clipboardData", {
    value: { getData: (kind: string) => (kind === "text/plain" ? text : "") },
  });
  await act(async () => void field().dispatchEvent(e));
  return e.defaultPrevented;
}

/** A pasted brief of exactly `n` characters, its words five letters and a space; `mark` sets it apart. */
const brief = (n: number, mark = "a") => `${mark}word `.repeat(Math.ceil(n / 6)).slice(0, n);

test("a paste at PASTE_CHIP_MIN is a chip, the field left empty; one character under it goes into the field", async () => {
  const onInput = vi.fn();
  await mount(composer({ onInput }));
  expect(await paste(brief(PASTE_CHIP_MIN - 1))).toBe(false);
  expect(chips()).toHaveLength(0);
  const big = brief(PASTE_CHIP_MIN);
  expect(await paste(big)).toBe(true);
  expect(field().value).toBe("");
  expect(chips()).toHaveLength(1);
  expect(chips()[0]!.textContent).toBe("Pasted text3,334 words");
  expect(onInput).toHaveBeenLastCalledWith(big, false);
  // Ask takes the chip alone
  expect(document.querySelector<HTMLButtonElement>('[data-testid="send"]')!.disabled).toBe(false);
});

test("the chip's size in words is grouped as the app writes counts (money.ts), and a one-word paste says 1 word", async () => {
  await mount(composer({}));
  await paste(brief(60_000));
  await paste("7".repeat(PASTE_CHIP_MIN));
  expect(chips().map((c) => c.querySelector(".q-paper-pb")?.textContent)).toEqual([
    "10,000 words",
    "1 word",
  ]);
  expect(wordCount("  one\ttwo\nthree\u00a0four  ")).toBe(4);
});

test("Ask sends the typed words, then each chip's text in paste order, a blank line between", async () => {
  const onAsk = vi.fn();
  await mount(composer({ onAsk }));
  const one = brief(PASTE_CHIP_MIN, "a");
  const two = brief(PASTE_CHIP_MIN + 5, "b");
  await paste(one);
  await paste(two);
  await type("Summarise both briefs.");
  await act(async () => document.querySelector<HTMLButtonElement>('[data-testid="send"]')!.click());
  expect(onAsk).toHaveBeenCalledExactlyOnceWith(`Summarise both briefs.\n\n${one}\n\n${two}`);
  // Enter the same
  await act(async () => {
    field().dispatchEvent(
      new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true }),
    );
  });
  expect(onAsk).toHaveBeenLastCalledWith(`Summarise both briefs.\n\n${one}\n\n${two}`);
});

test("Remove pasted text takes its chip off, tells the room, and puts the focus on the next chip, then the field", async () => {
  const onInput = vi.fn();
  const ref = createRef<ComposerHandle>();
  await mount(composer({ onInput, ref }));
  const one = brief(PASTE_CHIP_MIN, "a");
  const two = brief(PASTE_CHIP_MIN, "b");
  await paste(one);
  await paste(two);
  const buttons = chips().map((c) => c.querySelector("button")!);
  expect(buttons.map((b) => b.getAttribute("aria-label"))).toEqual([
    "Remove pasted text",
    "Remove pasted text",
  ]);
  // each button is described by its own chip's size
  expect(
    document.getElementById(buttons[0]!.getAttribute("aria-describedby")!)?.textContent,
  ).toMatch(/words$/);
  await act(async () => buttons[0]!.click());
  expect(chips()).toHaveLength(1);
  expect(onInput).toHaveBeenLastCalledWith(two, false);
  expect(ref.current?.text()).toBe(two);
  expect(document.activeElement).toBe(chips()[0]!.querySelector("button"));
  await act(async () => chips()[0]!.querySelector("button")!.click());
  expect(chips()).toHaveLength(0);
  expect(onInput).toHaveBeenLastCalledWith("", true);
  expect(document.activeElement).toBe(field());
});

test("a Backspace at the start of an empty field leaves the chips alone", async () => {
  await mount(composer({}));
  await paste(brief(PASTE_CHIP_MIN));
  field().focus();
  await act(async () => {
    field().dispatchEvent(
      new KeyboardEvent("keydown", { key: "Backspace", bubbles: true, cancelable: true }),
    );
  });
  expect(chips()).toHaveLength(1);
});

test("a big paste keeps the field small: its value never holds the pasted text", async () => {
  const onInput = vi.fn();
  await mount(composer({ onInput }));
  await paste(brief(6 * 1024 * 1024));
  await type("And this.");
  expect(field().value).toBe("And this.");
  expect(onInput.mock.lastCall?.[0]).toHaveLength(6 * 1024 * 1024 + "And this.\n\n".length);
});

test("the draft comes back with its chips: the typed words in the field, each chip as it was", async () => {
  const one = chipOf(brief(PASTE_CHIP_MIN, "a")).text;
  const two = chipOf(brief(PASTE_CHIP_MIN, "b")).text;
  const ref = createRef<ComposerHandle>();
  await mount(
    composer({ ref, initialText: askedOf("Compare these.", chipTail([chipOf(one), chipOf(two)])) }),
  );
  expect(field().value).toBe("Compare these.");
  expect(chips()).toHaveLength(2);
  expect(ref.current?.text()).toBe(`Compare these.\n\n${one}\n\n${two}`);
});

test("a long text with no chip remembered comes back as one chip, never into the field", async () => {
  const long = brief(PASTE_CHIP_MIN + 1, "z");
  await mount(composer({ initialText: long }));
  expect(field().value).toBe("");
  expect(chips()).toHaveLength(1);
  expect(splitAsked("short")).toEqual({ typed: "short", chips: [] });
});

test("fill, setText and clear carry the chips; Cmd+Z after a clear brings them back with the words", async () => {
  const ref = createRef<ComposerHandle>();
  const onInput = vi.fn();
  await mount(composer({ ref, onInput }));
  const one = brief(PASTE_CHIP_MIN, "a");
  await paste(one);
  await type("Read it.");
  const asked = ref.current!.text();
  expect(asked).toBe(`Read it.\n\n${one}`);
  // a clear for other words does nothing
  await act(async () => ref.current?.clear("Read it."));
  expect(chips()).toHaveLength(1);
  await act(async () => ref.current?.clear(asked));
  expect(chips()).toHaveLength(0);
  expect(field().value).toBe("");
  expect(onInput).toHaveBeenLastCalledWith("", true);
  // happy-dom has no execCommand, so the clear was made in place: the composer's own undo brings both back
  field().focus();
  const z = new KeyboardEvent("keydown", {
    key: "z",
    metaKey: true,
    bubbles: true,
    cancelable: true,
  });
  await act(async () => void field().dispatchEvent(z));
  expect(z.defaultPrevented).toBe(true);
  expect(chips()).toHaveLength(1);
  // the stopped question's refill: only into an empty composer, chips and all
  await act(async () => ref.current?.fill("Other words."));
  expect(field().value).not.toBe("Other words.");
  await act(async () => ref.current?.clear(ref.current.text()));
  await act(async () => ref.current?.fill(asked));
  expect(ref.current?.text()).toBe(asked);
  expect(chips()).toHaveLength(1);
  expect(field().value).toBe("Read it.");
  // setText with words appended after them, as a lost edit comes back
  await act(async () => ref.current?.setText(`${asked}\n\nAlso this.`));
  expect(chips()).toHaveLength(1);
  expect(field().value).toBe("Read it.\n\nAlso this.");
  expect(ref.current?.text()).toBe(`Read it.\n\nAlso this.\n\n${one}`);
});
