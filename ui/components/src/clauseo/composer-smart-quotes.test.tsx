// @vitest-environment happy-dom
// macOS's smart quotes on a quoted paper token (#117 M5, WKWebView): the composer refuses the system's replacement of a
// token's quote, as the pick puts it in and when a word is typed against it, and lets the lawyer's own quotes, Tamil99's
// vowel signs and input methods' compositions be.
import { act, createRef } from "react";
import { expect, test } from "vitest";
import type { ComposerHandle } from "./composer.tsx";
import {
  paper,
  PAPERS,
  source,
  composer,
  mount,
  field,
  names,
  focus,
  edit,
  key,
  compose,
  lay,
  boxes,
} from "./composer-mention-fixture.tsx";

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
