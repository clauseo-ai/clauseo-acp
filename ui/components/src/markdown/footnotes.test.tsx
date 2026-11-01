// @vitest-environment happy-dom
// Footnotes across blocks (footnotes.ts, answer.ts): numbering in order of first reference over the whole
// answer, a back-reference per repeat, multi-paragraph notes, one notes section at the end, ids scoped to
// the answer, pending numbers while streaming, literal text for an undefined reference at the end, and
// re-rendering only the blocks whose references change.
import { act } from "react";
import { createRoot } from "react-dom/client";
import type { Root } from "react-dom/client";
import { afterAll, beforeAll, expect, test } from "vitest";
import { createAnswer } from "./answer.ts";
import { LEGAL } from "./legal-fixture.ts";
import { Markdown } from "./markdown.tsx";
import { htmlOf, normalise, unsplit } from "./spec-fixture.ts";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement;
let root: Root;
beforeAll(() => {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterAll(() => {
  act(() => root.unmount());
  host.remove();
});

const render = (text: string, streaming = false) =>
  act(() => root.render(<Markdown text={text} streaming={streaming} />));

const target = (link: Element | null | undefined) =>
  document.getElementById(decodeURIComponent((link?.getAttribute("href") ?? "").slice(1)));

const DOC = [
  "First claim[^b] and a second[^a].",
  "",
  "Again[^b], and once more[^B].",
  "",
  "[^a]: Note A.",
  "",
  "[^b]: Note B, first paragraph.",
  "",
  "    Note B, second paragraph.",
  "",
].join("\n");

test("numbers follow the first reference across blocks; each reference links to its note and back", () => {
  render(DOC);
  const refs = [...host.querySelectorAll("p sup a")];
  expect(refs.map((ref) => ref.textContent)).toEqual(["1", "2", "1", "1"]);
  const notes = host.querySelectorAll("section[data-footnotes]");
  expect(notes).toHaveLength(1);
  expect(host.querySelector('[data-clauseo="markdown"]')?.lastElementChild).toBe(notes[0]);
  const items = [...notes[0]!.querySelectorAll("ol > li")];
  expect(items.map((item) => item.querySelector("p")?.textContent?.startsWith("Note B"))).toEqual([
    true,
    false,
  ]);
  // Every reference reaches its note; the note of b has one back-reference per reference, each to its own.
  expect(refs.map((ref) => target(ref))).toEqual([items[0], items[1], items[0], items[0]]);
  const backB = [...items[0]!.querySelectorAll("a[href]")];
  expect(backB.map((back) => target(back))).toEqual([refs[0], refs[2], refs[3]]);
  expect(new Set(refs.map((ref) => ref.id)).size).toBe(4);
  const backA = [...items[1]!.querySelectorAll("a[href]")];
  expect(backA.map((back) => target(back))).toEqual([refs[1]]);
  // A note of two paragraphs keeps both, its back-references after the last.
  expect(items[0]!.querySelectorAll("p")).toHaveLength(2);
  expect(items[0]!.querySelectorAll("p")[1]?.textContent).toMatch(/^Note B, second paragraph\./);
  // Every id carries the sanitiser's prefix and the answer's scope.
  const ids = [...host.querySelectorAll("[id]")].map((el) => el.id);
  expect(ids.length).toBe(4 + 2 + 1);
  for (const id of ids) expect(id).toMatch(/^user-content-md[\w-]+-(fnref|fn|footnote)-/);
  const scope = /^user-content-(md[\w-]*?)-fn/.exec(items[0]!.id)?.[1];
  expect(ids.every((id) => id.startsWith(`user-content-${scope}-`))).toBe(true);
});

test("a footnoted answer stays split: the legal memo is more than a hundred blocks, plus its notes", () => {
  const views = createAnswer("t").update(LEGAL, false);
  expect(views.length).toBeGreaterThan(100);
  expect(views.filter((view) => view.notes)).toHaveLength(1);
  expect(views.at(-1)?.notes).toBe(true);
});

test.each([
  ["repeats and two paragraphs", DOC],
  ["a note referenced inside another note", "A[^x].\n\n[^x]: See also[^y].\n\n[^y]: The other.\n"],
  ["definitions in a block quote and before the reference", "> [^q]: Quoted.\n\nText[^q].\n"],
  [
    "an unreferenced note, and a duplicate definition",
    "T[^a].\n\n[^a]: First.\n\n[^b]: Unused.\n\n[^a]: Second.\n",
  ],
  ["an undefined reference", "Known[^a] and unknown[^zz].\n\n[^a]: A.\n"],
  [
    "a note with a list and code",
    "T[^n].\n\n[^n]: Items:\n\n    - one\n    - two\n\n        code\n",
  ],
  [
    "a reference in a table and a heading",
    "# Title[^h]\n\n| a |\n| - |\n| b[^h] |\n\n[^h]: Note.\n",
  ],
  [
    "a link inside a note, defined elsewhere",
    "T[^n].\n\n[^n]: See [the Act].\n\n[the act]: https://example.com/act\n",
  ],
])("rendered block by block, %s renders as the whole answer does in one piece", (_, text) => {
  const views = createAnswer("spec").update(text, false);
  expect(views.filter((view) => !view.notes).length).toBeGreaterThan(1);
  expect(normalise(htmlOf(views))).toBe(normalise(unsplit(text)));
});

test("while streaming, a reference without its note is a pending number; at the end it is text", () => {
  const text = "A claim[^x] here.\n\nMore text";
  render(text, true);
  const pending = host.querySelector("sup[data-footnote-pending]");
  expect(pending?.textContent).toBe("1");
  expect(host.textContent).not.toContain("[^");
  render(text, false);
  expect(host.querySelector("sup")).toBeNull();
  expect(host.textContent).toContain("A claim[^x] here.");
});

test.each([
  ["an undefined reference", "> [^x] is cited in this quoted passage.", "[^x] is cited"],
  ["a label with a space (no reference)", "> [^a b] quote with a spaced label", "[^a b] quote"],
])(
  "a block quote of the agent's that starts with %s stays, streaming and finished",
  (_, text, shown) => {
    for (const before of ["", "Before.\n\n"])
      for (const streaming of [true, false]) {
        const html = htmlOf(createAnswer("t").update(`${before}${text}`, streaming));
        expect(html, `${before}${streaming}`).toContain("<blockquote>");
        // While streaming, the undefined reference is a pending number.
        expect(html).toContain(streaming && shown.startsWith("[^x]") ? "is cited" : shown);
      }
  },
);

test("a half-arrived reference at the tail is hidden, and a complete one is a pending number", () => {
  render("Earlier.\n\nThe indemnity survives.[^surv", true);
  expect(host.textContent).toBe("Earlier.The indemnity survives.");
  render("Earlier.\n\nThe indemnity survives.[^survival] The", true);
  expect(host.querySelector("sup[data-footnote-pending]")?.textContent).toBe("1");
  expect(host.textContent).toBe("Earlier.The indemnity survives.1 The");
});

test("once its note starts to arrive, a reference links to it, and the note streams in the notes", () => {
  const before = "Claim[^n].\n\nMore.\n\n[^n]: The no";
  render(before, true);
  const ref = host.querySelector("p sup a");
  expect(ref?.textContent).toBe("1");
  expect(target(ref)?.textContent).toContain("The no");
  render(`${before}te in full.`, false);
  expect(target(host.querySelector("p sup a"))?.textContent).toContain("The note in full.");
});

test("when a note arrives, only the blocks that reference it get a new view", () => {
  const paragraphs = Array.from({ length: 20 }, (_, index) =>
    index === 3 || index === 10 ? `Paragraph ${index} cites[^n].` : `Paragraph ${index}.`,
  );
  const body = `${paragraphs.join("\n\n")}\n\n`;
  const answer = createAnswer("t");
  const first = answer.update(`${body}Tail`, true);
  expect(first[3]?.hast).toBeDefined();
  const second = answer.update(`${body}[^n]: The note`, true);
  const changed = second.flatMap((view, index) =>
    index < 20 && view !== first[index] ? [index] : [],
  );
  expect(changed).toEqual([3, 10]);
  // The note growing, and more text after it, re-render no block that cites it.
  const third = answer.update(`${body}[^n]: The note grows.\n\nAnother paragraph`, true);
  expect(third.slice(0, 20).every((view, index) => view === second[index])).toBe(true);
  // A new reference to the same note in a later block re-renders neither earlier citing block.
  const fourth = answer.update(
    `${body}[^n]: The note grows.\n\nAnother paragraph cites[^n] too.\n\nNext`,
    true,
  );
  expect(fourth.slice(0, 21).every((view, index) => view === third[index])).toBe(true);
  expect(fourth.at(-1)?.notes).toBe(true);
});

test("a note's own block is parsed as it is, with no placeholder for the label it defines", () => {
  const views = createAnswer("t").update("Claim[^a].\n\n[^a]: The note cites[^a] itself.\n", true);
  const note = views.find((view) => view.source.startsWith("[^a]:"));
  expect(note?.input).toBe(note?.source);
});

test("a label that looks defined but sits in code still gets its placeholder, so it shows a number", () => {
  const text = "- See[^a].\n\n  ```\n  [^a]: not a note\n  ```\n\nMore";
  render(text, true);
  expect(host.querySelector("sup[data-footnote-pending]")?.textContent).toBe("1");
  expect(host.textContent).not.toContain("See[^a]");
});

test("streaming, a block calling more than 64 undefined labels shows the rest as written until a note arrives", () => {
  const refs = Array.from({ length: 70 }, (_, index) => `[^n${index}]`).join(" ");
  const answer = createAnswer("t");
  const before = htmlOf(answer.update(`${refs}\n\nTail`, true));
  expect(before.match(/data-footnote-pending/g)).toHaveLength(64);
  expect(before).toContain("[^n64]");
  const after = htmlOf(answer.update(`${refs}\n\n[^n69]: Late note.\n\nTail`, true));
  expect(after).toContain('href="#t-fn-n69"');
});

test("a finished answer re-rendered with no change keeps every view", () => {
  const answer = createAnswer("t");
  const first = answer.update(DOC, false);
  expect(answer.update(`${DOC}`, false)).toBe(first);
});
