// @vitest-environment happy-dom
// "Open in Word" over FakeTransport: a .doc or .docx the wall lists opens through open_file with its file ref, from its
// tile (a press, Enter or Space, with the hint on hover and keyboard focus and the label for screen readers) and from
// a change note naming it while the changes stand; never for a .pdf, a .txt or a paper the wall doesn't list. A second
// press while an open is on its way sends nothing. A declined open is quiet; a failed one says why under the note, and
// only in the console for a tile. An open Word started in Protected View says so in a quiet line under the note, until
// the next open or Undo.
import { IconFileGeneric, IconFilePdf, IconFileWord } from "@clauseo/components/icons/icons.tsx";
import { iconIn } from "@clauseo/components/icons/icon-in.tsx";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import axe from "axe-core";
import { act } from "react";
import { settled } from "@clauseo/components/lib/late.ts";
import { errorWords } from "@clauseo/components/lib/words-errors.ts";
import { rdt } from "@clauseo/components/lib/words-reader.ts";
import { afterEach, beforeAll, expect, test, vi } from "vitest";
import { reset, step, uuid } from "../../testing/stream-script.ts";
import type { FileChange, PaperSummary, ViewEntry } from "../../transport/protocol.ts";
import { fakeNext } from "../fake-next.ts";
import { defaultOutcome, mountRoom, type Harness, type Script } from "../thread/harness.ts";

let h: Harness | null = null;
beforeAll(() => {
  // the page's own title, as index.html gives it, for axe's document-title rule
  const html = readFileSync(
    join(dirname(fileURLToPath(import.meta.url)), "../../../index.html"),
    "utf8",
  );
  document.title = /<title>([^<]*)<\/title>/.exec(html)?.[1] ?? "";
});
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

test("every tile carries its file type's glyph by extension, at 24 px, and the Word tiles still open in Word", async () => {
  h = await withPapers();
  const glyph = (name: string) => faceOf(name)?.querySelector(".tile-glyph");
  expect(iconIn(faceOf("Reply.docx"), IconFileWord, 24, "tile-glyph")).not.toBeNull();
  expect(iconIn(faceOf("Old notice.DOC"), IconFileWord, 24, "tile-glyph")).not.toBeNull();
  expect(iconIn(faceOf("Exhibit.pdf"), IconFilePdf, 24, "tile-glyph")).not.toBeNull();
  expect(iconIn(faceOf("brief.txt"), IconFileGeneric, 24, "tile-glyph")).not.toBeNull();
  for (const name of ["Reply.docx", "Exhibit.pdf"])
    expect(glyph(name)?.getAttribute("aria-hidden")).toBe("true");
  expect(h.all("open-in-word").map((b) => b.textContent)).toEqual(["Reply.docx", "Old notice.DOC"]);
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

for (const language of ["en", "hi"] as const) {
  test(`axe finds no violation with a Word tile focused and its tooltip open: it mounts inside the wall (${language})`, async () => {
    h = await withPapers({ display: { language } });
    const face = faceOf("Reply.docx");
    await act(async () => {
      face?.dispatchEvent(new KeyboardEvent("keydown", { key: "Tab", bubbles: true }));
      face?.focus();
    });
    const label = rdt(language, "Open in Word");
    const popup = () =>
      [...document.body.querySelectorAll<HTMLElement>("[data-side]")].find(
        (n) => n.textContent === label,
      );
    await act(async () => vi.waitFor(() => expect(popup()).toBeDefined()));
    const results = await axe.run(document, { resultTypes: ["violations"] });
    expect(
      results.violations.map(
        (v) => `${v.id} at ${v.nodes.map((n) => n.target.join(" ")).join(", ")}`,
      ),
    ).toEqual([]);
    expect(popup()?.closest("aside.room-pane")).not.toBeNull();
    // the first tile, on the wall's top row: placed against the window, so the wall's scroll doesn't cut it off
    expect(tiles()[0]?.contains(face ?? null)).toBe(true);
    // its positioner, the first [data-side] holding the words
    expect(popup()?.getAttribute("role")).toBe("presentation");
    expect(popup()?.style.position).toBe("fixed");
  });
}

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

test("two quick presses on a Word tile send one open_file; once it answers, the next press opens again", async () => {
  h = await withPapers();
  const answers: (() => void)[] = [];
  h.transport.host.openFile = () => new Promise<void>((resolve) => void answers.push(resolve));
  await h.click(faceOf("Reply.docx"));
  await h.click(faceOf("Reply.docx"));
  expect(opened()).toEqual([uuid(1, 8)]);
  // another file isn't held back by it
  await h.click(faceOf("Old notice.DOC"));
  expect(opened()).toEqual([uuid(1, 8), uuid(2, 8)]);
  await act(async () => answers.forEach((answer) => answer()));
  await h.settle();
  h.transport.host.openFile = () => {};
  await h.click(faceOf("Reply.docx"));
  expect(opened()).toEqual([uuid(1, 8), uuid(2, 8), uuid(1, 8)]);
});

test("two quick presses on the note's Open in Word send one open_file", async () => {
  h = await withPapers();
  h.transport.host.openFile = () => new Promise<void>(() => {});
  h.deliver([reset([changed(["drafts/Reply.docx"])])]);
  const open = () =>
    h!.one("files-changed")?.querySelector<HTMLElement>('[data-testid="open-in-word"]');
  await h.click(open());
  await h.click(open());
  expect(opened()).toEqual([uuid(1, 8)]);
});

const noteError = () =>
  h!.one("files-changed")?.querySelector<HTMLElement>('[data-testid="error"]') ?? null;

for (const code of [
  "internal",
  "command.conflict",
  "matter.folder_missing",
  "command.invalid",
  "matter.document_path_too_long",
]) {
  test(`a failed open from the note says why in its error line, by its code: ${code}`, async () => {
    h = await withPapers();
    vi.spyOn(console, "warn").mockImplementation(() => {});
    h.transport.host.openFile = () => {
      throw { code, message: "for logs", retryable: false };
    };
    h.deliver([reset([changed(["drafts/Reply.docx"])])]);
    await h.click(h.one("files-changed")?.querySelector('[data-testid="open-in-word"]'));
    await h.settle();
    expect(noteError()?.dataset["code"]).toBe(code);
    expect(noteError()?.textContent).toBe(errorWords("en", code));
    // a press that opens clears it
    h.transport.host.openFile = () => {};
    await h.click(h.one("files-changed")?.querySelector('[data-testid="open-in-word"]'));
    await h.settle();
    expect(noteError()).toBeNull();
  });
}

const PROTECTED =
  "This file's path is long, so Word opens it in Protected View. Choose Enable Editing to make changes.";
const protectedLine = () =>
  h!.one("files-changed")?.querySelector<HTMLElement>('[data-testid="open-protected-view"]') ??
  null;
const openFromNote = async () => {
  await h!.click(h!.one("files-changed")?.querySelector('[data-testid="open-in-word"]'));
  await h!.settle();
};

test("an open in Protected View says so in a quiet line under the note; the next open clears it", async () => {
  h = await withPapers();
  h.transport.host.openFile = () => ({ protected_view: true });
  h.deliver([reset([changed(["drafts/Reply.docx"])])]);
  expect(protectedLine()).toBeNull();
  await openFromNote();
  expect(protectedLine()?.textContent).toBe(PROTECTED);
  expect(protectedLine()?.getAttribute("role")).toBe("status");
  expect(protectedLine()?.closest('[role="alert"]')).toBeNull();
  expect(noteError()).toBeNull();
  // the next open clears it as it goes, and an open outside Protected View leaves it gone
  h.transport.host.openFile = () => ({ protected_view: false });
  await openFromNote();
  expect(protectedLine()).toBeNull();
});

test("an open outside Protected View, or a failed one, shows no Protected View line", async () => {
  h = await withPapers();
  vi.spyOn(console, "warn").mockImplementation(() => {});
  h.deliver([reset([changed(["drafts/Reply.docx"])])]);
  await openFromNote();
  expect(opened()).toHaveLength(1);
  expect(protectedLine()).toBeNull();
  h.transport.host.openFile = () => {
    throw { code: "matter.document_path_too_long", message: "for logs", retryable: false };
  };
  await openFromNote();
  expect(protectedLine()).toBeNull();
  expect(noteError()?.dataset["code"]).toBe("matter.document_path_too_long");
});

test("Undo clears the Protected View line", async () => {
  h = await withPapers();
  h.transport.host.openFile = () => ({ protected_view: true });
  h.deliver([reset([changed(["drafts/Reply.docx"])])]);
  await openFromNote();
  expect(protectedLine()).not.toBeNull();
  await h.click(h.one("undo"));
  await h.settle();
  expect(protectedLine()).toBeNull();
});

test("a tile's open in Protected View says nothing in the room (it has no place to)", async () => {
  h = await withPapers();
  h.transport.host.openFile = () => ({ protected_view: true });
  await h.click(faceOf("Reply.docx"));
  await h.settle();
  expect(opened()).toEqual([uuid(1, 8)]);
  expect(h.host.querySelector('[data-testid="open-protected-view"]')).toBeNull();
});

test("a declined open from the note says nothing", async () => {
  h = await withPapers();
  h.transport.host.openFile = () => {
    throw { code: "transport.unauthorized", message: "declined", retryable: false };
  };
  h.deliver([reset([changed(["drafts/Reply.docx"])])]);
  await h.click(h.one("files-changed")?.querySelector('[data-testid="open-in-word"]'));
  await h.settle();
  expect(noteError()).toBeNull();
  expect(opened()).toHaveLength(1);
});

test("Open in Word stays on the note while the lock's lines stand", async () => {
  h = await withPapers({
    dispatch: (command) => {
      if (command.undo_step !== undefined)
        throw { code: "matter.file_locked", message: "held", retryable: true };
      return defaultOutcome(command);
    },
  });
  h.deliver([reset([changed(["drafts/Reply.docx"])])]);
  await h.click(h.one("undo"));
  await act(settled);
  await h.settle();
  expect(h.one("word-lock")).not.toBeNull();
  const open = h.one("files-changed")?.querySelector<HTMLElement>('[data-testid="open-in-word"]');
  expect(open?.textContent).toBe("Open in Word");
  await h.click(open);
  expect(opened()).toEqual([uuid(1, 8)]);
});
