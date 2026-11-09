// @vitest-environment happy-dom
// A Word document compared with now: the host's Word tool marks up its changes (word_blackline), set as the blackline
// sheet under the two times; a line while it runs; why it failed, over the sheet that says only that it changed, with
// Try again only where another try may mend it; a line when the host cut the document short; a marking up kept for
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
