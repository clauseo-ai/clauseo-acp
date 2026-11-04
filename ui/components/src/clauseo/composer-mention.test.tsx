// @vitest-environment happy-dom
// The composer's `@` menu at its interface, through the composer: it asks the room for its papers as `@` opens it (and
// again on each new `@`, the room caching), says it is loading before the first answer, filters as the lawyer types,
// picks with Enter into a token (`@path`, quoted when the path holds a space), never opens while an input method
// composes, and tells the handle which papers the field's tokens mention: each once, in order, none once deleted, again
// after undo.
import { act, createRef, type ComponentType } from "react";
import { createRoot } from "react-dom/client";
import { expect, test, vi } from "vitest";
import type { IconProps } from "../icons/icon.tsx";
import {
  IconFileEmail,
  IconFileGeneric,
  IconFileMarkdown,
  IconFilePdf,
  IconFileWord,
} from "../icons/icons.tsx";
import type { ComposerHandle, MentionList } from "./composer.tsx";
import {
  unmount,
  paper,
  PAPERS,
  source,
  composer,
  mount,
  field,
  menu,
  rows,
  names,
  notes,
  said,
  focus,
  edit,
  key,
  put,
  compose,
  lay,
  boxes,
  engine,
} from "./composer-mention-fixture.tsx";

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
  await unmount();
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
