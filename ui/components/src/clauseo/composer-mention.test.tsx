// @vitest-environment happy-dom
// The composer's `@` menu at its interface, through the composer: it asks the room for its papers as `@` opens it (and
// again on each new `@`, the room caching), says it is loading before the first answer, filters as the lawyer types,
// picks with Enter into a token (`@path`, quoted when the path holds a space), never opens while an input method
// composes, and tells the handle which papers the field's tokens mention: each once, in order, none once deleted, again
// after undo.
import { act, createRef, StrictMode, type ComponentType, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, test, vi } from "vitest";
import { settled } from "../lib/late.ts";
import { DEFAULT_DISPLAY, showDisplay } from "../lib/display.ts";
import type { IconProps } from "../icons/icon.tsx";
import {
  IconFileEmail,
  IconFileGeneric,
  IconFileMarkdown,
  IconFilePdf,
  IconFileWord,
} from "../icons/icons.tsx";
import { TooltipProvider } from "../ui/tooltip.tsx";
import {
  Composer,
  type ComposerHandle,
  type ComposerProps,
  type MentionItem,
  type MentionList,
  type Mentions,
} from "./composer.tsx";
import { RoomScopeProvider } from "./room-scope-provider.tsx";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
if (!("fonts" in document)) {
  Object.defineProperty(document, "fonts", {
    value: { ready: Promise.resolve(), status: "loaded", load: async () => [] },
  });
}

const paper = (id: string, path: string): MentionItem => {
  const cut = path.lastIndexOf("/");
  return {
    id,
    label: cut === -1 ? path : path.slice(cut + 1),
    detail: cut === -1 ? undefined : path.slice(0, cut),
    text: path,
  };
};
const PAPERS: MentionItem[] = [
  paper("f-1", "Lease deed.pdf"),
  paper("f-2", "Pleadings/Board minutes.pdf"),
  paper("f-3", "Notice.pdf"),
];

/** A room's papers: answers `list` (or what `answer` gives), counting the loads. */
function source(list: MentionList | (() => Promise<MentionList>) = { items: PAPERS }) {
  const load = vi.fn(typeof list === "function" ? list : async () => list);
  const mentions: Mentions = { load };
  return { mentions, load };
}

let host: HTMLDivElement | null = null;
let root: ReturnType<typeof createRoot> | null = null;
const wrap = (node: ReactNode) => (
  <StrictMode>
    <TooltipProvider>
      <RoomScopeProvider>{node}</RoomScopeProvider>
    </TooltipProvider>
  </StrictMode>
);
const composer = (p: Partial<ComposerProps>) => (
  <Composer
    took={null}
    plan=""
    hint={null}
    testIds={{ field: "composer", menu: "composer-menu", menuItem: "composer-menu-item" }}
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

afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  root = null;
  showDisplay(DEFAULT_DISPLAY);
  Reflect.deleteProperty(document, "execCommand");
  vi.useRealTimers();
  vi.restoreAllMocks();
});

const field = () => document.querySelector<HTMLTextAreaElement>('[data-testid="composer"]')!;
const menu = () => document.querySelector<HTMLElement>('[data-testid="composer-menu"]');
const rows = () => [
  ...document.querySelectorAll<HTMLElement>('[data-testid="composer-menu-item"]'),
];
const names = () => rows().map((r) => r.querySelector("bdi")?.textContent);
/** The plane's lines under the rows: loading, failed, none, the count past 50, a list that stops short. */
const notes = () => [...document.querySelectorAll(".fixed.z-20 p")].map((p) => p.textContent);
const said = () => document.querySelector('[aria-live="polite"]')?.textContent;

const focus = async () => {
  await act(async () => field().focus());
  await act(settled);
};
const edit = (value: string, init: { inputType?: string; isComposing?: boolean } = {}) =>
  act(async () => {
    const a = field();
    Reflect.set(HTMLTextAreaElement.prototype, "value", value, a);
    a.setSelectionRange(value.length, value.length);
    a.dispatchEvent(
      new InputEvent("input", {
        bubbles: true,
        inputType: init.inputType ?? "insertText",
        isComposing: init.isComposing ?? false,
      }),
    );
  });
const key = (k: string, init: KeyboardEventInit = {}) =>
  act(async () => {
    const e = new KeyboardEvent("keydown", { key: k, bubbles: true, cancelable: true, ...init });
    field().dispatchEvent(e);
    return e.defaultPrevented;
  });
const put = (at: number) =>
  act(async () => {
    field().setSelectionRange(at, at);
    document.dispatchEvent(new Event("selectionchange"));
  });
const compose = (kind: "compositionstart" | "compositionend") =>
  act(async () => {
    field().dispatchEvent(new CompositionEvent(kind, { bubbles: true, data: "" }));
  });

/** A Range's rects as a page would give them (composer-tokens.test.tsx's): one line, 10 px a character. */
function lay() {
  vi.spyOn(Range.prototype, "getClientRects").mockImplementation(function (this: Range) {
    const r = new DOMRect(this.startOffset * 10, 0, (this.endOffset - this.startOffset) * 10, 26);
    return [r] as unknown as DOMRectList;
  });
}
const boxes = () => [...document.querySelectorAll(".room-field-tokens .room-token")];

/** The engine's own editing with its undo stack (composer-tokens.test.tsx's). */
function engine() {
  const back: string[] = [];
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
    const kind = command === "delete" ? "deleteContentBackward" : "insertText";
    set(a, a.value.slice(0, s) + insert + a.value.slice(a.selectionEnd), s + insert.length, kind);
    return true;
  };
  Object.defineProperty(document, "execCommand", { value: exec, configurable: true });
  return {
    undo: () =>
      act(async () => {
        const v = back.pop();
        if (v !== undefined) set(field(), v, v.length, "historyUndo");
      }),
  };
}

test("@ asks the room for its papers as it opens, says it is loading, then lists them; Enter there never sends", async () => {
  let answer: (list: MentionList) => void = () => {};
  const { mentions, load } = source(() => new Promise((r) => (answer = r)));
  const onAsk = vi.fn();
  await mount(composer({ mentions, onAsk }));
  const a = field();
  // papers alone give the field its list
  expect(a.getAttribute("aria-haspopup")).toBe("listbox");
  await focus();
  expect(load).not.toHaveBeenCalled();
  await edit("@");
  expect(load).toHaveBeenCalledTimes(1);
  expect(notes()).toEqual(["Loading papers…"]);
  expect(said()).toBe("Loading papers…");
  expect(menu()).toBeNull();
  expect(a.hasAttribute("data-menu")).toBe(true);
  expect(a.hasAttribute("aria-activedescendant")).toBe(false);
  expect(await key("Enter", { keyCode: 13 })).toBe(true);
  expect(onAsk).not.toHaveBeenCalled();
  await act(async () => answer({ items: PAPERS }));
  expect(menu()?.getAttribute("aria-label")).toBe("Papers");
  expect(names()).toEqual(["Lease deed.pdf", "Board minutes.pdf", "Notice.pdf"]);
  expect(rows()[1]?.textContent).toContain("Pleadings");
  expect(said()).toBe("3 papers");
  expect(a.getAttribute("aria-activedescendant")).toBe(rows()[0]?.id);
  expect(notes()).toEqual([]);
});

test("a paper row's icon is its file type's, 20 px, hidden from assistive technology; its name is as before", async () => {
  const { mentions } = source({
    items: [
      paper("f-1", "X.docx"),
      paper("f-2", "X.pdf"),
      paper("f-3", "x.MD"),
      paper("f-4", "x.eml"),
      paper("f-5", "x.bin"),
    ],
  });
  await mount(composer({ mentions }));
  await focus();
  await edit("@");
  expect(names()).toEqual(["X.docx", "X.pdf", "x.MD", "x.eml", "x.bin"]);
  expect(rows().map((r) => r.textContent)).toEqual(["X.docx", "X.pdf", "x.MD", "x.eml", "x.bin"]);
  const glyphs = [IconFileWord, IconFilePdf, IconFileMarkdown, IconFileEmail, IconFileGeneric];
  for (const [i, r] of rows().entries()) {
    const svg = r.querySelector("svg");
    expect(svg?.getAttribute("aria-hidden")).toBe("true");
    expect(svg?.hasAttribute("aria-label")).toBe(false);
    expect(svg?.getAttribute("width")).toBe("20");
    expect(svg?.getAttribute("class")).toContain("group-data-highlighted:text-primary");
    expect(svg?.innerHTML).toBe(await drawn(glyphs[i]!));
  }
});

/** A glyph's markup at 20 px, drawn the way the row draws it. */
async function drawn(Glyph: ComponentType<IconProps>): Promise<string> {
  const box = document.createElement("div");
  const r = createRoot(box);
  await act(async () => r.render(<Glyph size={20} />));
  const html = box.querySelector("svg")?.innerHTML ?? "";
  await act(async () => r.unmount());
  return html;
}

test("the filter narrows as the lawyer types; a query matching nothing closes it; an email address opens nothing", async () => {
  const { mentions, load } = source();
  await mount(composer({ mentions }));
  await focus();
  await edit("rohan@clauseo");
  expect(load).not.toHaveBeenCalled();
  await edit("see @");
  await edit("see @no");
  expect(names()).toEqual(["Notice.pdf"]);
  await edit("see @mi");
  expect(names()).toEqual(["Board minutes.pdf"]);
  await edit("see @pleadings");
  expect(names()).toEqual(["Board minutes.pdf"]);
  await edit("see @zzz");
  expect(document.querySelector(".fixed.z-20")).toBeNull();
  // typing on within the same @ asked nothing more of the room
  expect(load).toHaveBeenCalledTimes(1);
});

test("Enter picks a paper as a token, quoted when its path holds a space; the handle names the papers once, in order", async () => {
  engine();
  const { mentions } = source();
  const handle = createRef<ComposerHandle>();
  const onAsk = vi.fn();
  await mount(composer({ mentions, ref: handle, onAsk }));
  await focus();
  expect(handle.current?.mentioned()).toEqual([]);
  await edit("compare @mi");
  expect(await key("Enter", { keyCode: 13 })).toBe(true);
  expect(onAsk).not.toHaveBeenCalled();
  expect(field().value).toBe('compare @"Pleadings/Board minutes.pdf" ');
  expect(document.querySelector(".fixed.z-20")).toBeNull();
  await edit('compare @"Pleadings/Board minutes.pdf" with @le');
  await key("Tab");
  expect(field().value).toBe('compare @"Pleadings/Board minutes.pdf" with @"Lease deed.pdf" ');
  // the same paper again: one mention
  await edit(`${field().value}and @mi`);
  await key("Enter", { keyCode: 13 });
  expect(handle.current?.mentioned().map((m) => m.id)).toEqual(["f-2", "f-1"]);
  // a token's own @ opens nothing
  await put(8);
  expect(document.querySelector(".fixed.z-20")).toBeNull();
  // with no menu open, Enter sends the words
  await put(field().value.length);
  expect(await key("Enter", { keyCode: 13 })).toBe(true);
  expect(onAsk).toHaveBeenCalledOnce();
});

test("a deleted token mentions nothing; undo brings it back, and its paper with it", async () => {
  const { undo } = engine();
  const { mentions } = source();
  const handle = createRef<ComposerHandle>();
  await mount(composer({ mentions, ref: handle }));
  await focus();
  await edit("see @no");
  await key("Enter", { keyCode: 13 });
  expect(field().value).toBe("see @Notice.pdf ");
  expect(handle.current?.mentioned().map((m) => m.id)).toEqual(["f-3"]);
  await put(15);
  expect(await key("Backspace")).toBe(true);
  expect(field().value).toBe("see  ");
  expect(handle.current?.mentioned()).toEqual([]);
  await undo();
  expect(field().value).toBe("see @Notice.pdf ");
  expect(handle.current?.mentioned().map((m) => m.id)).toEqual(["f-3"]);
  // a character typed into the token unmakes it: the words alone, no mention
  await edit("see @Notixce.pdf ");
  expect(handle.current?.mentioned()).toEqual([]);
});

/** The engine as WKWebView on macOS edits, smart quotes on (#117 M5-hi's log): insertText's beforeinput, then the words
 * in, then, inside the same command, the closing `"` replaced by `”` with insertReplacementText (its own beforeinput,
 * the caret at the words' end, no target ranges), then insertText's input and the replacement's. `cancelable: false`
 * is an engine that never asks before it substitutes. */
function smartQuotes({ cancelable = true } = {}) {
  const exec = (command: string, _ui?: boolean, value?: string) => {
    const a = document.activeElement;
    if (!(a instanceof HTMLTextAreaElement) || command !== "insertText" || value === undefined)
      return false;
    const before = (inputType: string, data: string) =>
      a.dispatchEvent(
        new InputEvent("beforeinput", { bubbles: true, cancelable, inputType, data }),
      );
    if (!before("insertText", value)) return false;
    const s = a.selectionStart;
    const v = a.value.slice(0, s) + value + a.value.slice(a.selectionEnd);
    Reflect.set(HTMLTextAreaElement.prototype, "value", v, a);
    a.setSelectionRange(s + value.length, s + value.length);
    const q = value.lastIndexOf('"');
    const replaced = q > 0 && before("insertReplacementText", "\u201d");
    if (replaced) {
      const at = s + q;
      const caret = a.selectionEnd;
      Reflect.set(
        HTMLTextAreaElement.prototype,
        "value",
        `${v.slice(0, at)}\u201d${v.slice(at + 1)}`,
        a,
      );
      a.setSelectionRange(caret, caret);
    }
    a.dispatchEvent(
      new InputEvent("input", { bubbles: true, inputType: "insertText", data: value }),
    );
    if (replaced)
      a.dispatchEvent(
        new InputEvent("input", {
          bubbles: true,
          inputType: "insertReplacementText",
          data: "\u201d",
        }),
      );
    return true;
  };
  Object.defineProperty(document, "execCommand", { value: exec, configurable: true });
}
const ANNEXURE = paper("f-a", "papers/Annexure A अनुलग्नक.pdf");
/** A replacement asked for at beforeinput over `start` to `end` selected, as WebKit selects what it replaces:
 * whether the composer let it be. */
const replacing = (start: number, end: number, data: string) =>
  act(async () => {
    field().setSelectionRange(start, end);
    const e = new InputEvent("beforeinput", {
      bubbles: true,
      cancelable: true,
      inputType: "insertReplacementText",
      data,
    });
    return field().dispatchEvent(e);
  });

test("macOS's smart quotes never rewrite a quoted paper's closing quote as it is picked (#117 M5-hi, WKWebView)", async () => {
  lay();
  smartQuotes();
  const { mentions } = source({ items: [ANNEXURE, ...PAPERS] });
  const handle = createRef<ComposerHandle>();
  await mount(composer({ mentions, ref: handle }));
  await focus();
  await edit("आदेश @ann");
  expect(names()).toEqual(["Annexure A अनुलग्नक.pdf"]);
  await key("Tab");
  const picked = 'आदेश @"papers/Annexure A अनुलग्नक.pdf" ';
  expect(field().value).toBe(picked);
  expect([field().selectionStart, field().selectionEnd]).toEqual([39, 39]);
  expect(boxes()).toHaveLength(1);
  expect(handle.current?.mentioned().map((m) => m.id)).toEqual(["f-a"]);
  // a replacement over the token's characters (a spelling fix in its name) is refused; one at its end or after it,
  // as Tamil99's vowel sign after a token, is let be
  expect(await replacing(11, 19, "Annex")).toBe(false);
  expect(await replacing(37, 38, "\u201d")).toBe(false);
  // a later smart-quote pass asks as the pick's did, the caret after the space, collapsed (review of #117 M5): refused
  expect(await replacing(39, 39, "\u201d")).toBe(false);
  expect(await replacing(38, 39, "x")).toBe(true);
  await edit(`${picked}க`);
  // a curly quote at a caret with no straight one since the token can only be the token's: refused
  expect(await replacing(40, 40, "\u201d")).toBe(false);
  // the lawyer's own straight quote after the token, and an apostrophe, are theirs to have curled: let be
  await edit(`${picked}க "`);
  expect(await replacing(42, 42, "\u201d")).toBe(true);
  await edit(`${picked}don'`);
  expect(await replacing(43, 43, "\u2019")).toBe(true);
  await edit(`${picked}க`);
  expect(await replacing(39, 40, "கா")).toBe(true);
  await edit(`${picked}கா`, { inputType: "insertReplacementText" });
  expect(boxes()).toHaveLength(1);
  expect(handle.current?.mentioned().map((m) => m.id)).toEqual(["f-a"]);
});

test("a word typed against a picked paper's closing quote keeps the quote straight and the token (#117 M5-hi step 10, WKWebView)", async () => {
  lay();
  smartQuotes();
  const { mentions } = source({ items: [ANNEXURE, ...PAPERS] });
  const handle = createRef<ComposerHandle>();
  await mount(composer({ mentions, ref: handle }));
  await focus();
  await edit("आदेश @ann");
  await key("Tab");
  const picked = 'आदेश @"papers/Annexure A अनुलग्नक.pdf" ';
  expect(field().value).toBe(picked);
  // the caret against the closing quote (Left), then Hindi Transliteration's word, as the log has it: the composition
  // in, then WebKit's smart quote with the caret after the word, flagged as no composition, before compositionend
  await act(async () => field().setSelectionRange(38, 38));
  await compose("compositionstart");
  const typed = 'आदेश @"papers/Annexure A अनुलग्नक.pdf"न्यायालय  ';
  await act(async () => {
    const a = field();
    Reflect.set(HTMLTextAreaElement.prototype, "value", typed, a);
    a.setSelectionRange(47, 47);
    a.dispatchEvent(
      new InputEvent("input", {
        bubbles: true,
        inputType: "insertFromComposition",
        isComposing: true,
      }),
    );
  });
  expect(await replacing(47, 47, "\u201d")).toBe(false);
  await compose("compositionend");
  expect(field().value).toBe(typed);
  // typed against, the token is plain text while it is; a space between makes it a token again
  await edit('आदेश @"papers/Annexure A अनुलग्नक.pdf" न्यायालय  ');
  expect(boxes()).toHaveLength(1);
  expect(handle.current?.mentioned().map((m) => m.id)).toEqual(["f-a"]);
});

test("an engine that substitutes without asking leaves the pick's words once, never part of them twice", async () => {
  smartQuotes({ cancelable: false });
  const { mentions } = source({ items: [ANNEXURE] });
  await mount(composer({ mentions }));
  await focus();
  await edit("आदेश @ann");
  await key("Tab");
  expect(field().value).toBe('आदेश @"papers/Annexure A अनुलग्नक.pdf\u201d ');
});

test("no @ menu opens, and no papers are asked for, while an input method composes; an @ in it opens at its end", async () => {
  vi.useFakeTimers({ toFake: ["setTimeout"] });
  const { mentions, load } = source();
  await mount(composer({ mentions }));
  await focus();
  await compose("compositionstart");
  await edit("@", { inputType: "insertCompositionText", isComposing: true });
  await edit("@उ", { inputType: "insertCompositionText" });
  expect(document.querySelector(".fixed.z-20")).toBeNull();
  expect(load).not.toHaveBeenCalled();
  await compose("compositionend");
  expect(load).not.toHaveBeenCalled();
  await act(async () => vi.runAllTimers());
  expect(load).toHaveBeenCalledTimes(1);
  // nothing matches उ here, so the menu stays shut; the room was asked once the composition ended
  await edit("@");
  expect(names()).toEqual(["Lease deed.pdf", "Board minutes.pdf", "Notice.pdf"]);
});

test("each new @ asks the room again, the last list standing meanwhile; a failed first read says so", async () => {
  let fail = true;
  const { mentions, load } = source(async () => {
    if (fail) throw new Error("refused");
    return { items: PAPERS };
  });
  await mount(composer({ mentions }));
  await focus();
  await edit("@");
  expect(notes()).toEqual(["Couldn't read this matter's papers."]);
  await key("Escape");
  fail = false;
  await edit("@ @");
  expect(load).toHaveBeenCalledTimes(2);
  expect(names()).toHaveLength(3);
  // a third @ whose read fails keeps the list it had
  fail = true;
  await edit("@ @ @");
  expect(load).toHaveBeenCalledTimes(3);
  expect(names()).toHaveLength(3);
  expect(notes()).toEqual([]);
});

test("a matter with no papers says so; more than 50 matches show 50 with the count; a list that stops short says so", async () => {
  const empty = source({ items: [] });
  await mount(composer({ mentions: empty.mentions }));
  await focus();
  await edit("@");
  expect(notes()).toEqual(["No papers yet."]);
  await act(async () => root?.unmount());
  host?.remove();
  const many = Array.from({ length: 2000 }, (_, i) => paper(`f-${i}`, `Bundle/Exhibit ${i}.pdf`));
  const full = source({ items: many, partial: true });
  await mount(composer({ mentions: full.mentions }));
  await focus();
  await edit("@exhibit");
  expect(rows()).toHaveLength(50);
  expect(said()).toBe("2000 papers");
  expect(notes()).toEqual([
    "First 50 of 2000. Type to narrow.",
    "Not every file in this folder is listed.",
  ]);
  // the lines stand under the rows, out of their scroll: only the rows scroll
  const lines = [...document.querySelectorAll(".fixed.z-20 p")];
  expect(lines.every((p) => menu()?.contains(p) === false)).toBe(true);
  expect(menu()?.classList.contains("overflow-y-auto")).toBe(true);
  expect(menu()?.parentElement?.classList.contains("overflow-hidden")).toBe(true);
  // in their own space under the rows, parted from them by the menus' divide
  const foot = menu()?.nextElementSibling;
  expect(foot?.firstElementChild?.classList.contains("bg-divide-mid")).toBe(true);
  expect(lines.every((p) => foot?.contains(p))).toBe(true);
});

test("a paper token typed against at its end or its start is plain text: no box, and no paper for the send", async () => {
  lay();
  engine();
  const { mentions } = source();
  const handle = createRef<ComposerHandle>();
  await mount(composer({ mentions, ref: handle }));
  await focus();
  await edit("see @no");
  await key("Enter", { keyCode: 13 });
  await edit("see @Notice.pdf now");
  expect(boxes()).toHaveLength(1);
  expect(handle.current?.mentioned().map((m) => m.id)).toEqual(["f-3"]);
  // the space after it gone and a letter typed there
  await edit("see @Notice.pdfx now");
  expect(boxes()).toHaveLength(0);
  expect(handle.current?.mentioned()).toEqual([]);
  // and again, typed against at its start
  await edit("see @no");
  await key("Enter", { keyCode: 13 });
  expect(boxes()).toHaveLength(1);
  await edit("see y@Notice.pdf ");
  expect(boxes()).toHaveLength(0);
  expect(handle.current?.mentioned()).toEqual([]);
  // nor does it delete as one unit, or keep the caret out: Backspace at its end takes one character
  await put(16);
  expect(await key("Backspace")).toBe(false);
  await put(10);
  expect(field().selectionStart).toBe(10);
});

test("undo never brings back a picked paper from inside another paper's quoted name", async () => {
  const { undo } = engine();
  const b = paper("f-b", "b.pdf");
  const p = paper("f-p", "x @b.pdf y");
  const { mentions } = source({ items: [b, p] });
  const handle = createRef<ComposerHandle>();
  await mount(composer({ mentions, ref: handle }));
  await focus();
  await edit("@b.");
  await key("Enter", { keyCode: 13 });
  expect(field().value).toBe("@b.pdf ");
  expect(handle.current?.mentioned().map((m) => m.id)).toEqual(["f-b"]);
  // the other paper's quoted name typed in by hand over it (never picked), then cleared, then undone back
  await edit('@"x @b.pdf y" ', { inputType: "insertFromPaste" });
  await act(async () => handle.current?.clear());
  expect(field().value).toBe("");
  await undo();
  expect(field().value).toBe('@"x @b.pdf y" ');
  expect(handle.current?.mentioned()).toEqual([]);
});

test("the IME guard in the menu's input listener alone: an input flagged as composing opens nothing", async () => {
  // no compositionstart reached the composer (the composition began before the field had the focus), so its own
  // record says nothing composes: only the event's flag holds the query
  const { mentions, load } = source();
  await mount(composer({ mentions }));
  await focus();
  await edit("@", { inputType: "insertCompositionText", isComposing: true });
  expect(document.querySelector(".fixed.z-20")).toBeNull();
  expect(load).not.toHaveBeenCalled();
});

test("the IME guard in the menu's refresh alone: a selection change mid-composition reads no query", async () => {
  const { mentions } = source();
  await mount(composer({ mentions }));
  await focus();
  await edit("@");
  expect(names()).toHaveLength(3);
  await compose("compositionstart");
  // the composition's text in the field, its caret moving: the open menu is left as it was until compositionend
  await act(async () => {
    Reflect.set(HTMLTextAreaElement.prototype, "value", "@zzz", field());
    field().setSelectionRange(4, 4);
    document.dispatchEvent(new Event("selectionchange"));
  });
  expect(names()).toHaveLength(3);
});
