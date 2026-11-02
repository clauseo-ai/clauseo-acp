// @vitest-environment happy-dom
// The composer's tokens at its interface, through the composer and its `/` menu: a pick is a token, drawn as a box on
// the overlay behind the field; it deletes as one unit, the caret never rests inside it, undo and redo read the words
// again, a command the agent no longer lists is plain text, and the overlay copies the field's face and follows its
// scroll. happy-dom lays nothing out, so where the boxes stand is checked against the rects a Range is made to give.
import { act, createRef, StrictMode, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, test, vi } from "vitest";
import { settled } from "../lib/late.ts";
import { DEFAULT_DISPLAY, showDisplay } from "../lib/display.ts";
import { TooltipProvider } from "../ui/tooltip.tsx";
import {
  Composer,
  type ComposerHandle,
  type ComposerProps,
  type SlashCommand,
} from "./composer.tsx";
import { RoomScopeProvider } from "./room-scope-provider.tsx";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
if (!("fonts" in document)) {
  Object.defineProperty(document, "fonts", {
    value: { ready: Promise.resolve(), status: "loaded", load: async () => [] },
  });
}

const COMMANDS: SlashCommand[] = [
  { name: "review", description: "Review the changes" },
  { name: "restart", description: "Start the session again" },
  { name: "compact", description: "Compact the conversation" },
];

let host: HTMLDivElement | null = null;
let root: ReturnType<typeof createRoot> | null = null;
const wrap = (node: ReactNode) => (
  <StrictMode>
    <TooltipProvider>
      <RoomScopeProvider>{node}</RoomScopeProvider>
    </TooltipProvider>
  </StrictMode>
);
const composer = (p: Partial<ComposerProps> = {}) => (
  <Composer
    took={null}
    plan=""
    hint={null}
    testIds={{ field: "composer", menu: "composer-menu", menuItem: "composer-menu-item" }}
    commands={COMMANDS}
    {...p}
  />
);
async function mount(node: ReactNode) {
  showDisplay({ ...DEFAULT_DISPLAY, motion: "reduced" });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root?.render(wrap(node)));
}
const rerender = (node: ReactNode) => act(async () => root?.render(wrap(node)));

afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  root = null;
  showDisplay(DEFAULT_DISPLAY);
  Reflect.deleteProperty(document, "execCommand");
  vi.restoreAllMocks();
});

const field = () => document.querySelector<HTMLTextAreaElement>('[data-testid="composer"]')!;
const menu = () => document.querySelector('[data-testid="composer-menu"]');
const overlay = () => document.querySelector<HTMLElement>(".room-field-tokens")!;
const boxes = () => [...overlay().querySelectorAll<HTMLElement>(".room-token")];
/** Where each box starts, in characters: the Range stand-in below draws a character 10 px wide, on one line. */
const starts = () => boxes().map((b) => parseFloat(b.style.left) / 10);

/** A Range's rects as a page would give them, one line, 10 px a character, 26 px a line. */
function lay() {
  vi.spyOn(Range.prototype, "getClientRects").mockImplementation(function (this: Range) {
    const r = new DOMRect(this.startOffset * 10, 0, (this.endOffset - this.startOffset) * 10, 26);
    return [r] as unknown as DOMRectList;
  });
}

const focus = async () => {
  await act(async () => field().focus());
  await act(settled);
};
const edit = (value: string, caret = value.length, inputType = "insertText") =>
  act(async () => {
    const a = field();
    Reflect.set(HTMLTextAreaElement.prototype, "value", value, a);
    a.setSelectionRange(caret, caret);
    a.dispatchEvent(new InputEvent("input", { bubbles: true, inputType }));
  });
const key = (k: string, init: KeyboardEventInit = {}) =>
  act(async () => {
    const e = new KeyboardEvent("keydown", { key: k, bubbles: true, cancelable: true, ...init });
    field().dispatchEvent(e);
    return e.defaultPrevented;
  });
const caret = () => [field().selectionStart, field().selectionEnd];
/** The caret put somewhere, as a press or Up and Down would, and the page telling of it. */
const put = (start: number, end = start) =>
  act(async () => {
    field().setSelectionRange(start, end);
    document.dispatchEvent(new Event("selectionchange"));
  });

/** The engine's own editing with its undo stack, as WebKit and Chromium keep one: insertText and delete replace the
 * selection and say so; undo and redo put the words back with historyUndo and historyRedo. */
function engine() {
  const back: string[] = [];
  const ahead: string[] = [];
  const set = (a: HTMLTextAreaElement, v: string, at: number, inputType: string) => {
    Reflect.set(HTMLTextAreaElement.prototype, "value", v, a);
    a.setSelectionRange(at, at);
    a.dispatchEvent(new InputEvent("input", { bubbles: true, inputType }));
  };
  const exec = (command: string, _ui?: boolean, value?: string) => {
    const a = document.activeElement;
    if (!(a instanceof HTMLTextAreaElement)) return false;
    const insert = command === "delete" ? "" : (value ?? "");
    const s = a.selectionStart;
    back.push(a.value);
    ahead.length = 0;
    const kind = command === "delete" ? "deleteContentBackward" : "insertText";
    set(a, a.value.slice(0, s) + insert + a.value.slice(a.selectionEnd), s + insert.length, kind);
    return true;
  };
  Object.defineProperty(document, "execCommand", { value: exec, configurable: true });
  const history = (from: string[], to: string[], inputType: string) =>
    act(async () => {
      const a = field();
      const v = from.pop();
      if (v === undefined) return;
      to.push(a.value);
      set(a, v, v.length, inputType);
    });
  return {
    undo: () => history(back, ahead, "historyUndo"),
    redo: () => history(ahead, back, "historyRedo"),
  };
}

/** `/re` typed in the field and /review picked with Enter. */
const pickReview = async (before = "") => {
  await edit(`${before}/re`);
  expect(menu()).not.toBeNull();
  await key("Enter");
  expect(field().value).toBe(`${before}/review `);
};

test("a pick is a token: the overlay holds the words as one text node and draws one box behind the command", async () => {
  lay();
  engine();
  await mount(composer());
  await focus();
  expect(boxes()).toEqual([]);
  await pickReview("ask ");
  expect(starts()).toEqual([4]);
  expect(parseFloat(boxes()[0]!.style.width)).toBe(70);
  // the words themselves: one run, never split around the token, drawn transparent by the stylesheet
  const texts = [...overlay().childNodes].filter((n) => n.nodeType === Node.TEXT_NODE);
  expect(texts.map((n) => n.textContent)).toEqual(["ask /review \u200b"]);
  expect(overlay().getAttribute("aria-hidden")).toBe("true");
  // words typed before and after it move it, and keep it
  await edit("please ask /review ", 7);
  expect(starts()).toEqual([11]);
  await edit("please ask /review now");
  expect(starts()).toEqual([11]);
  // and one typed into it drops it
  await edit("please ask /revview now", 15);
  expect(starts()).toEqual([]);
});

test("Backspace right after a token, or Delete right before it, takes it whole, through the engine; undo brings it back", async () => {
  lay();
  const { undo } = engine();
  await mount(composer());
  await focus();
  await pickReview("ask ");
  await put(11);
  expect(await key("Backspace")).toBe(true);
  expect(field().value).toBe("ask  ");
  expect(boxes()).toEqual([]);
  await undo();
  expect(field().value).toBe("ask /review ");
  expect(starts()).toEqual([4]);
  await put(4);
  expect(await key("Delete")).toBe(true);
  expect(field().value).toBe("ask  ");
  // not with a modifier held: that is the engine's own word delete
  await undo();
  await put(11);
  expect(await key("Backspace", { altKey: true })).toBe(false);
  // and never while an input method composes
  expect(await key("Backspace", { isComposing: true })).toBe(false);
  expect(await key("Backspace", { keyCode: 229 })).toBe(false);
  expect(field().value).toBe("ask /review ");
});

test("the caret steps over a token and never rests inside one; a selection grows to whole tokens", async () => {
  lay();
  engine();
  await mount(composer());
  await focus();
  await pickReview("ask ");
  await put(11);
  expect(await key("ArrowLeft")).toBe(true);
  expect(caret()).toEqual([4, 4]);
  expect(await key("ArrowRight")).toBe(true);
  expect(caret()).toEqual([11, 11]);
  // put inside by a press: to the nearer edge
  await put(6);
  expect(caret()).toEqual([4, 4]);
  await put(9);
  expect(caret()).toEqual([11, 11]);
  await put(1, 6);
  expect(caret()).toEqual([1, 11]);
  // a caret beside it stays
  await put(12);
  expect(caret()).toEqual([12, 12]);
});

test("undo goes back to the words before the pick, no token; redo brings the token back", async () => {
  lay();
  const { undo, redo } = engine();
  await mount(composer());
  await focus();
  await pickReview();
  expect(starts()).toEqual([0]);
  await undo();
  expect(field().value).toBe("/re");
  expect(boxes()).toEqual([]);
  await redo();
  expect(field().value).toBe("/review ");
  expect(starts()).toEqual([0]);
});

test("a command the agent no longer lists is plain text; one the field held as the menu came is a token", async () => {
  lay();
  engine();
  await mount(composer());
  await focus();
  await pickReview();
  await rerender(composer({ commands: COMMANDS.slice(1) }));
  expect(boxes()).toEqual([]);
  await put(4);
  expect(caret()).toEqual([4, 4]);
  await act(async () => root?.unmount());
  host?.remove();
  // a draft that holds a command, as the composer opens on it
  await mount(composer({ initialText: "/compact the thread, /nothing" }));
  await focus();
  expect(starts()).toEqual([0]);
});

test("a picked command's slash opens no menu as the caret comes back to it", async () => {
  lay();
  engine();
  const commands = [...COMMANDS, { name: "reviews", description: "Every review" }];
  await mount(composer({ commands }));
  await focus();
  await edit("/revi");
  await key("Enter");
  expect(field().value).toBe("/review ");
  await edit("/reviews ", 8);
  expect(menu()).toBeNull();
});

test("words set in place by the composer, with no input event, drop the tokens they replace", async () => {
  lay();
  engine();
  const handle = createRef<ComposerHandle>();
  await mount(composer({ ref: handle }));
  await focus();
  await pickReview();
  // the lawyer has gone on to another control: the clear happens in place
  const other = document.createElement("input");
  document.body.append(other);
  await act(async () => other.focus());
  await act(async () => handle.current?.clear());
  expect(field().value).toBe("");
  expect(boxes()).toEqual([]);
  other.remove();
});

test("the overlay copies the field's face, padding, line height and transform, takes its client width, and follows its scroll", async () => {
  lay();
  engine();
  await mount(composer());
  const a = field();
  Object.assign(a.style, {
    fontFamily: '"Anek Devanagari", sans-serif',
    fontSize: "17px",
    fontWeight: "500",
    lineHeight: "26px",
    paddingLeft: "10px",
    paddingTop: "1px",
    letterSpacing: "normal",
    transform: "translateY(1.5px)",
  });
  Object.defineProperty(a, "clientWidth", { value: 480, configurable: true });
  Object.defineProperty(a, "clientHeight", { value: 156, configurable: true });
  await focus();
  await pickReview();
  const o = overlay();
  expect(o.style.fontFamily).toBe(a.style.fontFamily);
  expect(o.style.fontSize).toBe("17px");
  expect(o.style.fontWeight).toBe("500");
  expect(o.style.lineHeight).toBe("26px");
  expect(o.style.paddingLeft).toBe("10px");
  expect(o.style.paddingTop).toBe("1px");
  expect(o.style.transform).toBe("translateY(1.5px)");
  expect(o.style.width).toBe("480px");
  expect(o.style.height).toBe("156px");
  // at the six lines' cap the field scrolls, and the overlay with it
  await act(async () => {
    a.scrollTop = 52;
    a.dispatchEvent(new Event("scroll"));
  });
  expect(o.scrollTop).toBe(a.scrollTop);
});
