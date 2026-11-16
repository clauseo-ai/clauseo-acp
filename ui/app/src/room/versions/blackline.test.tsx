// @vitest-environment happy-dom
// A Word document compared with now: the host's Word tool marks up its changes (word_blackline), set as the blackline
// sheet under the two times; a line while it runs; why it failed, over the sheet that says only that it changed, with
// Try again only where another try may mend it, and the Word tool's own refusals saying why; a line when the host cut
// the document short, and one for each notice (a part not shown, hidden text); a marking up kept for
// the same file and version, so coming back asks nothing; an answer that comes after the lawyer moved on, not shown;
// and every other file on the sheet it had.
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { MATTER, THREAD, TURN } from "../../testing/stream-script.ts";
import type { FileChange, HostQuery, QueryResult, ViewEntry } from "../../transport/protocol.ts";
import { file, mountVersions, NOON, stepEntry, version, type VersionsHarness } from "./testing.ts";

let h: VersionsHarness | null = null;
beforeEach(() => {
  vi.setSystemTime(NOON + 30 * 60_000);
});
afterEach(async () => {
  await h?.unmount();
  h = null;
  vi.useRealTimers();
});

const LEASE = file("Lease.docx", { before: "c-before", after: "c-after" });
const VERSIONS: ViewEntry[] = [
  version(1, "import"),
  version(2, "after_turn", { thread: THREAD, turn: TURN, files: [LEASE] }),
  version(3, "outside_edits", { files: [file("other.txt")] }),
];
const STEPS = [stepEntry(5, { files: [LEASE], checkpoint_after: "cp2" })];

const MARKED: QueryResult = {
  word_blackline: {
    paragraphs: [
      { change: "same", runs: [{ kind: "same", text: "The parties agree." }] },
      {
        change: "changed",
        runs: [
          { kind: "same", text: "Rent is due on the " },
          { kind: "del", text: "first" },
          { kind: "ins", text: "fifth" },
          { kind: "same", text: " day." },
        ],
      },
      { change: "added", runs: [{ kind: "ins", text: "A late fee applies." }] },
    ],
    truncated: false,
  },
};

const MARKING = "Marking up the changes…";
const NO_TOOL = "Clauseo's Word tool is missing. Reinstall Clauseo to see the marked-up changes.";
const FAILED = "The changes in this document couldn't be marked up.";
const CUT = "This document is very long, so only part of it is shown.";

/** A host error, as the transport rejects with it. */
const refusal = (code: string, retryable: boolean) => ({ code, message: "no", retryable });

/** A deferred answer, settled by hand. */
function later<T>() {
  let resolve: (v: T) => void = () => undefined;
  let reject: (e: unknown) => void = () => undefined;
  const promise = new Promise<T>((ok, no) => {
    resolve = ok;
    reject = no;
  });
  return { promise, resolve, reject };
}

/** Versions at cp2, compared with now, with compare_versions answering `changes` (or by the version compared) and word_blackline `mark`. */
async function comparing(
  mark: (
    q: Extract<HostQuery, { word_blackline: unknown }>["word_blackline"],
  ) => QueryResult | Promise<QueryResult>,
  changes: FileChange[] | ((from: string) => FileChange[]) = [LEASE],
): Promise<VersionsHarness> {
  const room = await mountVersions(VERSIONS, STEPS, {
    query: (q) => {
      if (typeof q !== "object") return undefined;
      if (q.compare_versions !== undefined)
        return {
          file_changes: typeof changes === "function" ? changes(q.compare_versions.from) : changes,
        };
      if (q.word_blackline !== undefined) return mark(q.word_blackline);
      return undefined;
    },
  });
  await room.openVersions();
  await room.lookAt("cp2");
  await room.click(room.button("Compare with now"));
  return room;
}

const page = () => document.body.querySelector<HTMLElement>(".ver-desk .page");
const fallback = () => document.body.querySelector<HTMLElement>(".ver-desk .ver-pdf");
const lines = () =>
  [...document.body.querySelectorAll<HTMLElement>(".ver-mark-text")].map((l) => l.textContent);

test("a Word document changed since is marked up by the host, set as the blackline under the two times", async () => {
  h = await comparing(() => MARKED);
  expect(h.queried("word_blackline")).toEqual([
    { word_blackline: { matter: MATTER, path: "Lease.docx", from: "cp2" } },
  ]);
  expect(fallback()).toBeNull();
  expect(page()?.querySelector(".page-title")?.textContent).toContain("Lease.docx");
  expect([...(page()?.querySelectorAll(".ver-ins") ?? [])].map((e) => e.textContent)).toEqual([
    "fifth",
    "A late fee applies.",
  ]);
  expect(page()?.querySelector(".ver-del")?.textContent).toBe("first");
  expect(page()?.querySelectorAll("[data-change]")).toHaveLength(2);
  const head = document.body.querySelector(".ver-marked-head");
  expect(head?.querySelector("[data-side='then']")?.textContent).toMatch(/^As it was at /);
  expect(head?.querySelector("[data-side='now']")?.textContent).toBe("As it is now");
  expect(lines()).toEqual([]);
});

test("while the host marks it up, the two times stand with a line saying so", async () => {
  const answer = later<QueryResult>();
  h = await comparing(() => answer.promise);
  expect(lines()).toEqual([MARKING]);
  expect(document.body.querySelector(".ver-marked-head")).not.toBeNull();
  expect(page()).toBeNull();
  answer.resolve(MARKED);
  await h.settle();
  expect(lines()).toEqual([]);
  expect(page()).not.toBeNull();
});

test("with the Word tool missing, the line says to reinstall, with no Try again, over the sheet that says it changed", async () => {
  h = await comparing(() => Promise.reject(refusal("blackline.no_word_tool", false)));
  expect(lines()).toEqual([NO_TOOL]);
  expect(h.button("Try again")).toBeNull();
  expect(fallback()?.textContent).toContain("Lease.docx");
  expect(page()).toBeNull();
});

test("a file the tool can't read says only that it couldn't be marked up, with no Try again", async () => {
  h = await comparing(() => Promise.reject(refusal("blackline.not_word", false)));
  expect(lines()).toEqual([FAILED]);
  expect(h.button("Try again")).toBeNull();
  expect(fallback()).not.toBeNull();
});

test("a marking up that failed says so with Try again, which asks again and shows the blackline", async () => {
  let n = 0;
  h = await comparing(() => {
    n += 1;
    return n === 1 ? Promise.reject(refusal("blackline.failed", true)) : MARKED;
  });
  expect(lines()).toEqual([FAILED]);
  expect(fallback()).not.toBeNull();
  await h.click(h.button("Try again"));
  expect(h.queried("word_blackline")).toHaveLength(2);
  expect(lines()).toEqual([]);
  expect(page()).not.toBeNull();
  expect(fallback()).toBeNull();
});

test("a document the host cut short says only part of it is shown, over the sheet", async () => {
  h = await comparing(() => ({ word_blackline: { ...MARKED.word_blackline!, truncated: true } }));
  expect(lines()).toEqual([CUT]);
  expect(page()).not.toBeNull();
});

test("a document shown in part, or with hidden text, says so, a line for each notice, over the sheet", async () => {
  h = await comparing(() => ({
    word_blackline: {
      ...MARKED.word_blackline!,
      notices: ["part_not_shown", "hidden_text", "numbers_may_differ", "may_differ", "unknown"],
    },
  }));
  expect(lines()).toEqual([
    "Part of this document couldn't be marked up. The rest is below.",
    "This document has text that Word hides. It's shown here like the rest.",
    "Some numbers here may not match the ones Word shows.",
    "Some of this may look different in Word.",
    "Some of this may look different in Word.",
  ]);
  expect(page()?.querySelectorAll("[data-change]")).toHaveLength(2);
  expect(fallback()).toBeNull();
});

test("a change with no text of its own shows its object's word, marked in or out", async () => {
  h = await comparing(() => ({
    word_blackline: {
      paragraphs: [
        {
          change: "changed",
          runs: [
            { kind: "same", text: "Signed: " },
            { kind: "ins", text: "", object: "picture" },
            { kind: "del", text: "", object: "note" },
            { kind: "ins", text: "", object: "unknown" },
          ],
        },
      ],
      truncated: false,
    },
  }));
  expect([...(page()?.querySelectorAll(".ver-ins") ?? [])].map((e) => e.textContent)).toEqual([
    "[picture]",
    "[object]",
  ]);
  expect(page()?.querySelector(".ver-del")?.textContent).toBe("[footnote]");
});

test("what the sheet doesn't mark says so: each changed link by its text, then a comment, a text box, cells", async () => {
  h = await comparing(() => ({
    word_blackline: {
      ...MARKED.word_blackline!,
      notices: ["comment_changed", "text_box_changed", "cell_format_changed"],
      changed_links: ["the account", "the bank's site"],
    },
  }));
  expect(lines()).toEqual([
    "The web address behind 'the account' changed.",
    "The web address behind 'the bank's site' changed.",
    "A comment's text changed. Open both versions in Word to see it.",
    "Text in a text box changed. Open both versions in Word to see it.",
    "The formatting of some tables, rows or cells changed.",
  ]);
  expect(page()?.querySelectorAll("[data-change]")).toHaveLength(2);
});

test("changes outside what's shown say so over the sheet, naming comments, not headers or footnotes", async () => {
  h = await comparing(() => ({
    word_blackline: { ...MARKED.word_blackline!, notices: ["changes_outside_text"] },
  }));
  expect(lines()).toEqual([
    "Some changes are outside what's shown here, such as in comments. Open both versions in Word to see them.",
  ]);
});

const MAY_DIFFER = "Some of this may look different in Word.";

test("a header or footer compare couldn't compare has a line of its own, not the 'may differ' line", async () => {
  h = await comparing(() => ({
    word_blackline: {
      ...MARKED.word_blackline!,
      notices: ["header_or_footer_not_compared", "may_differ"],
    },
  }));
  expect(lines()).toEqual([
    "A header or footer couldn't be compared, so changes in it aren't shown. Open both versions in Word to see them.",
    MAY_DIFFER,
  ]);
});

/** The headings after the body, and each part's inserted and deleted words. */
const partsShown = () =>
  [...(page()?.querySelectorAll<HTMLElement>(".ver-part") ?? [])].map((s) => ({
    heading: s.querySelector(".ver-part-title")?.textContent ?? null,
    ins: [...s.querySelectorAll(".ver-ins")].map((e) => e.textContent),
    del: [...s.querySelectorAll(".ver-del")].map((e) => e.textContent),
    barred: s.querySelectorAll("[data-change]").length,
  }));

const swap = (out: string, into: string) => ({
  change: "changed" as const,
  runs: [
    { kind: "same" as const, text: "See " },
    { kind: "del" as const, text: out },
    { kind: "ins" as const, text: into },
  ],
});

test("a footnote and a first-page header that changed are shown after the body, each under its heading, marked as the body is", async () => {
  h = await comparing(() => ({
    word_blackline: {
      ...MARKED.word_blackline!,
      parts: [
        { kind: { footnote: { number: 3 } }, paragraphs: [swap("3 March", "5 March")] },
        {
          kind: { header: { uses: [{ slot: "first_page", sections: [1] }] } },
          paragraphs: [
            { change: "same", runs: [{ kind: "same", text: "Lease" }] },
            swap("draft", "final"),
          ],
        },
        { kind: { endnote: { number: 1 } }, paragraphs: [swap("a", "b")] },
        {
          kind: { footer: { uses: [{ slot: "even_pages", sections: [1] }] } },
          paragraphs: [swap("c", "d")],
        },
        { kind: { header: { uses: [] } }, paragraphs: [swap("e", "f")] },
      ],
    },
  }));
  expect(partsShown()).toEqual([
    { heading: "Footnote 3", ins: ["5 March"], del: ["3 March"], barred: 1 },
    { heading: "Header (first page)", ins: ["final"], del: ["draft"], barred: 1 },
    { heading: "Endnote 1", ins: ["b"], del: ["a"], barred: 1 },
    { heading: "Footer (even pages)", ins: ["d"], del: ["c"], barred: 1 },
    { heading: "Header", ins: ["f"], del: ["e"], barred: 1 },
  ]);
  // the body comes first, as before, and keeps its own marks
  expect(page()?.querySelector(".page-paras")?.closest(".ver-part")).toBeNull();
  expect(page()?.querySelector(".ver-del")?.textContent).toBe("first");
  expect(lines()).toEqual([]);
});

test("two sections' headers on the same kind of page say their section; a header alone of its kind doesn't", async () => {
  const header = (slot: "default" | "first_page", n: number) => ({
    kind: { header: { uses: [{ slot, sections: [n, n + 1] }] } },
    paragraphs: [swap("x", "y")],
  });
  h = await comparing(() => ({
    word_blackline: {
      ...MARKED.word_blackline!,
      parts: [header("default", 1), header("default", 3), header("first_page", 3)],
    },
  }));
  expect(partsShown().map((p) => p.heading)).toEqual([
    "Header, section 1",
    "Header, section 3",
    "Header (first page)",
  ]);
});

test("the table notice names each table and what changed in it; past four, the first three and how many more", async () => {
  const first = { slot: "first_page" as const, sections: [1] };
  const four = [
    { place: "body" as const, number: 2, formats: ["borders" as const, "shading" as const] },
    { place: { header: { uses: [first] } }, number: 1, formats: ["merged_cells" as const] },
    { place: "footnotes" as const, number: 1, formats: ["row_height" as const] },
    { place: "endnotes" as const, number: 2, formats: ["other" as const, "unknown" as const] },
  ];
  h = await comparing(() => ({
    word_blackline: {
      ...MARKED.word_blackline!,
      notices: ["cell_format_changed"],
      changed_tables: four,
    },
  }));
  expect(lines()).toEqual([
    "Formatting changed in table 2 (borders, shading), Header (first page), table 1 (merged cells), table 1 in the footnotes (row height) and table 2 in the endnotes (other formatting).",
  ]);
  await h.unmount();
  h = await comparing(() => ({
    word_blackline: {
      ...MARKED.word_blackline!,
      notices: ["cell_format_changed"],
      changed_tables: [
        ...four,
        { place: "body", number: 5, formats: ["widths", "alignment"] },
        { place: "somewhere_new", number: 1, formats: ["borders"] },
      ],
    },
  }));
  expect(lines()).toEqual([
    "Formatting changed in table 2 (borders, shading), Header (first page), table 1 (merged cells), table 1 in the footnotes (row height) and 3 more.",
  ]);
});

test("a table notice with no table named, or only tables in places this version doesn't know, says only that formatting changed", async () => {
  h = await comparing(() => ({
    word_blackline: {
      ...MARKED.word_blackline!,
      notices: ["cell_format_changed"],
      changed_tables: [{ place: "somewhere_new", number: 1, formats: ["borders"] }],
    },
  }));
  expect(lines()).toEqual(["The formatting of some tables, rows or cells changed."]);
});

test("a place that couldn't be shown says so where it stood; a formatting change and hidden text are named over their words", async () => {
  h = await comparing(() => ({
    word_blackline: {
      paragraphs: [
        { change: "same", runs: [{ kind: "same", text: "The parties agree." }] },
        { change: "not_shown", runs: [] },
        {
          change: "formatted",
          runs: [{ kind: "same", text: "Rent is due monthly." }],
          hidden: true,
        },
      ],
      truncated: false,
    },
  }));
  const paras = [...(page()?.querySelectorAll<HTMLElement>(".ver-place") ?? [])];
  expect(paras.map((p) => p.dataset.change ?? null)).toEqual([null, "not_shown", "formatted"]);
  expect(paras[1]?.textContent).toBe("This part couldn't be shown.");
  expect(
    [...(paras[2]?.querySelectorAll(".ver-para-note") ?? [])].map((n) => n.textContent),
  ).toEqual(["Formatting changed", "Hidden in Word"]);
  expect(paras[2]?.textContent).toContain("Rent is due monthly.");
  expect(page()?.querySelectorAll(".ver-ins, .ver-del")).toHaveLength(0);
});

test.each([
  [
    "blackline.nested_table",
    "Clauseo couldn't compare these two versions because of a table inside a table. Open both in Word to compare them.",
  ],
  [
    "blackline.cannot_compare",
    "Clauseo couldn't compare these two versions. Open both in Word to compare them.",
  ],
  [
    "blackline.not_readable",
    "Clauseo couldn't read one of these versions. It may be damaged, or it may not be a Word document.",
  ],
  [
    "blackline.too_large",
    "This document is too large to mark up. Open both versions in Word to compare them.",
  ],
  [
    "blackline.format_unknown",
    "This version of Clauseo can't mark up these changes. Update Clauseo to see them.",
  ],
])(
  "the Word tool's own refusal %s says why, with no Try again, over the sheet that says it changed",
  async (code, line) => {
    h = await comparing(() => Promise.reject(refusal(code, false)));
    expect(lines()).toEqual([line]);
    expect(h.button("Try again")).toBeNull();
    expect(fallback()).not.toBeNull();
  },
);

test("coming back to the same file and version shows the blackline kept, asking the host nothing again", async () => {
  // cp1's comparison names another file, so the sheet in view there is not the Word document's
  h = await comparing(
    () => MARKED,
    (from) => (from === "cp2" ? [LEASE] : [file("other.txt")]),
  );
  expect(h.queried("word_blackline")).toHaveLength(1);
  await h.lookAt("cp1");
  expect(page()).toBeNull();
  await h.lookAt("cp2");
  // each look compares again: the host is asked what differs, never to mark the document up again
  expect(h.queried("compare_versions")).toHaveLength(3);
  expect(page()).not.toBeNull();
  expect(lines()).toEqual([]);
  expect(h.queried("word_blackline")).toHaveLength(1);
});

test("the file changing again since asks the host again", async () => {
  let lease = LEASE;
  h = await comparing(
    () => MARKED,
    (from) => (from === "cp2" ? [lease] : [file("other.txt")]),
  );
  lease = { ...LEASE, after: "c-newer" };
  await h.lookAt("cp1");
  await h.lookAt("cp2");
  expect(h.queried("word_blackline")).toHaveLength(2);
});

test("an answer that comes after the lawyer moved to another version is not shown there", async () => {
  const answers: ReturnType<typeof later<QueryResult>>[] = [];
  h = await comparing(() => {
    const a = later<QueryResult>();
    answers.push(a);
    return a.promise;
  });
  // cp1 compared with now: the same document, against another version
  await h.lookAt("cp1");
  expect(h.queried("word_blackline")).toHaveLength(2);
  answers[0]?.reject(refusal("blackline.failed", true));
  await h.settle();
  expect(lines()).toEqual([MARKING]);
  expect(h.button("Try again")).toBeNull();
  answers[1]?.resolve(MARKED);
  await h.settle();
  expect(page()).not.toBeNull();
  expect(lines()).toEqual([]);
});

test("a file that isn't a changed Word document keeps the sheet that says it changed, and nothing is marked up", async () => {
  h = await comparing(() => MARKED, [file("exhibit.pdf"), file("New.docx", { change: "added" })]);
  expect(fallback()?.textContent).toContain("exhibit.pdf");
  expect(h.queried("word_blackline")).toEqual([]);
});
