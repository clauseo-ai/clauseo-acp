// @vitest-environment happy-dom
// "Open in Word" over FakeTransport: a .doc or .docx the wall lists opens through open_file with its file ref, from its
// tile (a press, Enter or Space, with the hint on hover and keyboard focus and the label for screen readers) and from
// a change note naming it while the changes stand; never for a .pdf, a .txt or a paper the wall doesn't list. A
// declined or failed open is quiet.
import { act } from "react";
import { settled } from "@clauseo/components/lib/late.ts";
import { rdt } from "@clauseo/components/lib/words-reader.ts";
import { afterEach, expect, test, vi } from "vitest";
import { reset, step, uuid } from "../../testing/stream-script.ts";
import type { FileChange, PaperSummary, ViewEntry } from "../../transport/protocol.ts";
import { fakeNext } from "../fake-next.ts";
import { mountRoom, type Harness, type Script } from "../thread/harness.ts";

let h: Harness | null = null;
afterEach(async () => {
  await h?.unmount();
  h = null;
  document.body.replaceChildren();
  vi.restoreAllMocks();
});

const paper = (n: number, path: string): PaperSummary => ({
  file: uuid(n, 8),
  path,
  size: 1000,
  modified: 1_730_000_000_000,
});
const PAPERS = [
  paper(1, "drafts/Reply.docx"),
  paper(2, "Old notice.DOC"),
  paper(3, "Exhibit.pdf"),
  paper(4, "brief.txt"),
];

const file = (path: string): FileChange => ({
  path,
  change: "modified",
  size_bytes: 10,
  may_include_lawyer_edits: false,
});
const changed = (paths: string[], undone = false): ViewEntry =>
  step(9, { files: paths.map(file), checkpoint_after: uuid(1, 9), undone });

/** The room with H8's papers, and the reader's words loaded once a Word paper is listed. */
async function withPapers(script: Script = {}): Promise<Harness> {
  const room = await mountRoom({
    ...script,
    next: fakeNext({ matters: () => [], papers: PAPERS, parts: ["papers"] }),
  });
  await act(settled);
  await room.settle();
  return room;
}

const tiles = () => [...(h?.host.querySelectorAll("aside.room-pane li.tile") ?? [])];
const faceOf = (name: string) =>
  tiles()
    .find((t) => t.querySelector(".tile-title")?.textContent === name)
    ?.querySelector<HTMLButtonElement>("button.tile-face") ?? null;
const opened = () => h!.transport.only("open_file").map((c) => c.file);

test("a .doc and a .docx tile open in Word, by their file refs; a .pdf and a .txt tile don't", async () => {
  h = await withPapers();
  const word = h.all("open-in-word");
  expect(word.map((b) => b.textContent)).toEqual(["Reply.docx", "Old notice.DOC"]);
  expect(faceOf("Exhibit.pdf")?.dataset["testid"]).toBeUndefined();
  expect(faceOf("brief.txt")?.dataset["testid"]).toBeUndefined();
  await h.click(faceOf("Reply.docx"));
  await h.click(faceOf("Old notice.DOC"));
  await h.click(faceOf("Exhibit.pdf"));
  await h.click(faceOf("brief.txt"));
  await h.settle();
  expect(opened()).toEqual([uuid(1, 8), uuid(2, 8)]);
});

test("a Word tile says it opens in Word to screen readers, and keeps its hint out of the title", async () => {
  h = await withPapers({ display: { language: "hi" } });
  const face = faceOf("Reply.docx");
  const more = face?.getAttribute("aria-describedby");
  expect(more).toBeTruthy();
  expect(document.getElementById(more ?? "")?.textContent).toBe(rdt("hi", "Open in Word"));
  expect(face?.getAttribute("title")).toBeNull();
  // a tile that isn't Word's keeps no description
  expect(faceOf("Exhibit.pdf")?.getAttribute("aria-describedby")).toBeNull();
});

test("the hint shows on keyboard focus, in the tooltip, as on hover", async () => {
  h = await withPapers();
  const face = faceOf("Reply.docx");
  expect(tipText()).toBeNull();
  await act(async () => {
    face?.dispatchEvent(new KeyboardEvent("keydown", { key: "Tab", bubbles: true }));
    face?.focus();
  });
  await act(async () => vi.waitFor(() => expect(tipText()).toBe("Open in Word")));
});

const tipText = () =>
  [...document.body.querySelectorAll('[role="tooltip"], [data-side]')]
    .map((n) => n.textContent)
    .find((t) => t === "Open in Word") ?? null;

test("Enter and Space on a focused Word tile open it: the face is a native button with nothing in the way", async () => {
  h = await withPapers();
  const face = faceOf("Reply.docx");
  expect(face).toBeInstanceOf(HTMLButtonElement);
  expect(face?.type).toBe("button");
  for (const key of ["Enter", " "]) {
    const down = new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true });
    await act(async () => void face?.dispatchEvent(down));
    // the tile prevents neither key, so the browser's button activation runs, which is a click
    expect(down.defaultPrevented, key).toBe(false);
  }
  await h.click(face);
  expect(opened()).toEqual([uuid(1, 8)]);
});

test("a declined open says nothing; a failed one goes only to the console", async () => {
  h = await withPapers();
  const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
  h.transport.host.openFile = () => {
    throw { code: "transport.unauthorized", message: "declined", retryable: false };
  };
  await h.click(faceOf("Reply.docx"));
  await h.settle();
  expect(warn).not.toHaveBeenCalled();
  h.transport.host.openFile = () => {
    throw { code: "internal", message: "no", retryable: false };
  };
  await h.click(faceOf("Reply.docx"));
  await h.settle();
  expect(warn).toHaveBeenCalledTimes(1);
  expect(h.host.querySelector('[role="alert"]')).toBeNull();
  expect(opened()).toHaveLength(2);
});

test("a change note naming one listed Word document offers Open in Word, which opens it by its file ref", async () => {
  h = await withPapers();
  h.deliver([reset([changed(["drafts/Reply.docx", "brief.txt"])])]);
  const note = h.one("files-changed");
  const open = note?.querySelector<HTMLElement>('[data-testid="open-in-word"]');
  expect(open?.textContent).toBe("Open in Word");
  await h.click(open);
  expect(opened()).toEqual([uuid(1, 8)]);
});

for (const [name, paths, undone] of [
  ["a .pdf and a .txt", ["Exhibit.pdf", "brief.txt"], false],
  ["a Word document the wall doesn't list", ["Unlisted.docx"], false],
  ["two Word documents", ["drafts/Reply.docx", "Old notice.DOC"], false],
  ["a Word document whose change is undone", ["drafts/Reply.docx"], true],
] as const) {
  test(`a change note naming ${name} offers no Open in Word`, async () => {
    h = await withPapers();
    h.deliver([reset([changed([...paths], undone)])]);
    expect(h.one("files-changed")).not.toBeNull();
    expect(h.one("files-changed")?.querySelector('[data-testid="open-in-word"]')).toBeNull();
  });
}

test("without H8's list nothing offers Open in Word", async () => {
  h = await mountRoom();
  h.deliver([reset([changed(["drafts/Reply.docx"])])]);
  await act(settled);
  expect(h.one("files-changed")).not.toBeNull();
  expect(h.all("open-in-word")).toEqual([]);
});
