// @vitest-environment happy-dom
// A Word document's blackline as the host's Word tool marks it up: its paragraphs set on the sheet with words in and
// out marked, each paragraph that changed barred, none in the lawyer's weight, under the two times; then each part that
// changed (a footnote, a header) under its heading, its paragraphs set as the body's.
import { act, StrictMode, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, test } from "vitest";
import { MarkedHead, MarkedSheet, MarkLine } from "./version-page.tsx";
import { markedDiffs } from "./versions-diff.ts";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let root: ReturnType<typeof createRoot> | null = null;

async function show(node: ReactNode) {
  const host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root?.render(<StrictMode>{node}</StrictMode>));
}

afterEach(async () => {
  await act(async () => root?.unmount());
  root = null;
  document.body.replaceChildren();
});

const one = (selector: string) => {
  const n = document.querySelector<HTMLElement>(selector);
  if (n === null) throw new Error(`no ${selector}`);
  return n;
};
const texts = (selector: string) =>
  [...document.querySelectorAll<HTMLElement>(selector)].map((e) => e.textContent);

test("a Word document's blackline from the host's paragraphs: words in and out marked, added and removed paragraphs barred, none the lawyer's", async () => {
  const diffs = markedDiffs([
    { change: "same", runs: [{ kind: "same", text: "The parties agree as follows." }] },
    {
      change: "changed",
      runs: [
        { kind: "same", text: "Rent is due on the " },
        { kind: "del", text: "first" },
        { kind: "ins", text: "fifth" },
        { kind: "same", text: " day of each month." },
      ],
    },
    { change: "added", runs: [{ kind: "ins", text: "A late fee of 2% applies." }] },
    { change: "removed", runs: [{ kind: "del", text: "The deposit is waived." }] },
  ]);
  expect(diffs.map((d) => d.place)).toEqual([0, 1, 2, 3]);
  expect(diffs.flatMap((d) => d.runs).every((r) => !r.mine)).toBe(true);
  await show(
    <>
      <MarkedHead then="As it was at 3:42 pm on 9 March" now="As it is now" />
      <MarkLine text="This document is very long, so only part of it is shown." />
      <MarkedSheet title="Lease.docx" diffs={diffs} />
    </>,
  );
  expect(one(".page-title").textContent).toContain("Lease.docx");
  expect(texts(".page .ver-ins")).toEqual(["fifth", "A late fee of 2% applies."]);
  expect(texts(".page .ver-del")).toEqual(["first", "The deposit is waived."]);
  expect(document.querySelectorAll(".ver-mine")).toHaveLength(0);
  const marked = [...document.querySelectorAll<HTMLElement>(".page-paras p")].map(
    (p) => p.dataset["change"] ?? "same",
  );
  expect(marked).toEqual(["same", "changed", "added", "removed"]);
  expect(one(".ver-marked-head [data-side='then']").textContent).toContain(
    "As it was at 3:42 pm on 9 March",
  );
  expect(one(".ver-mark-line [role='status']").textContent).toContain(
    "This document is very long, so only part of it is shown.",
  );
});

test("a newer host's kinds: a paragraph changed in a way this version doesn't know reads as changed, its run as the same words", () => {
  expect(markedDiffs([{ change: "unknown", runs: [{ kind: "unknown", text: "x" }] }])).toEqual([
    { place: 0, kind: "changed", runs: [{ kind: "same", text: "x", mine: false }] },
  ]);
});

test("a formatting change and a place not shown read as changed with their note; hidden text is kept as a flag", () => {
  expect(
    markedDiffs([
      { change: "formatted", runs: [{ kind: "same", text: "x" }], hidden: true },
      { change: "not_shown", runs: [] },
    ]),
  ).toEqual([
    {
      place: 0,
      kind: "changed",
      runs: [{ kind: "same", text: "x", mine: false }],
      note: "formatted",
      hidden: true,
    },
    { place: 1, kind: "changed", runs: [], note: "not_shown" },
  ]);
});

test("a run that stands for an object keeps it; a text run has none", () => {
  expect(
    markedDiffs([
      {
        change: "changed",
        runs: [
          { kind: "same", text: "a", object: null },
          { kind: "ins", text: "", object: "picture" },
        ],
      },
    ]),
  ).toEqual([
    {
      place: 0,
      kind: "changed",
      runs: [
        { kind: "same", text: "a", mine: false },
        { kind: "ins", text: "", mine: false, object: "picture" },
      ],
    },
  ]);
});

test("a part after the body stands under its heading, its words in and out marked and its changed paragraph barred as the body's", async () => {
  const body = markedDiffs([
    { change: "same", runs: [{ kind: "same", text: "The parties agree." }] },
  ]);
  const note = markedDiffs([
    {
      change: "changed",
      runs: [
        { kind: "same", text: "See the statement of " },
        { kind: "del", text: "3 March" },
        { kind: "ins", text: "5 March" },
      ],
    },
  ]);
  const header = markedDiffs([
    { change: "added", runs: [{ kind: "ins", text: "Without prejudice" }] },
  ]);
  await show(
    <MarkedSheet
      title="Lease.docx"
      diffs={body}
      parts={[
        { heading: "Footnote 3", diffs: note },
        { heading: "Header (first page)", diffs: header },
        { heading: null, diffs: header },
      ]}
    />,
  );
  const parts = [...document.querySelectorAll<HTMLElement>(".page .ver-part")];
  expect(parts.map((p) => p.querySelector(".ver-part-title")?.textContent ?? null)).toEqual([
    "Footnote 3",
    "Header (first page)",
    null,
  ]);
  expect(parts[0]?.querySelector("h3")?.textContent).toBe("Footnote 3");
  expect([...(parts[0]?.querySelectorAll(".ver-del") ?? [])].map((e) => e.textContent)).toEqual([
    "3 March",
  ]);
  expect([...(parts[0]?.querySelectorAll(".ver-ins") ?? [])].map((e) => e.textContent)).toEqual([
    "5 March",
  ]);
  expect(parts[0]?.querySelector<HTMLElement>(".page-paras p")?.dataset["change"]).toBe("changed");
  expect(parts[1]?.querySelector<HTMLElement>(".page-paras p")?.dataset["change"]).toBe("added");
  // the body stands first, outside every part
  expect(one(".page > .page-paras").textContent).toBe("The parties agree.");
});
