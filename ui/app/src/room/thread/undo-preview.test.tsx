// @vitest-environment happy-dom
// Undo's ask on a press, as the host's PreviewUndo answers it: a paper edited since asks about its file name, nothing
// edited sends the undo at once, and a query that fails or is refused falls back to the flag a file carries. The press
// waits for the answer, and each press asks again.
import { act } from "react";
import { th } from "@clauseo/components/lib/words-thread.ts";
import { afterEach, expect, test, vi } from "vitest";
import { reset, step, THREAD, TURN, uuid } from "../../testing/stream-script.ts";
import type { FileChange, HostError, StepEntry, ViewEntry } from "../../transport/protocol.ts";
import { mountRoom, previewUndoScript, type Harness } from "./harness.ts";

let h: Harness | null = null;
afterEach(async () => {
  await h?.unmount();
  h = null;
  document.body.replaceChildren();
  vi.useRealTimers();
});

const file = (path: string, edited = false): FileChange => ({
  path,
  change: "modified",
  size_bytes: 10,
  may_include_lawyer_edits: edited,
});

const changed = (files: FileChange[], over: Partial<StepEntry> = {}): ViewEntry =>
  step(9, { files, checkpoint_after: uuid(1, 9), ...over });

/** PreviewUndo's answer, as the fake host gives it. */
const answer = (edited: string[]) => ({ undo_preview: { edited_since: edited } });

/** The ask's title for a paper, as the dialog sets its name apart. */
const askTitle = (name: string) =>
  th("en", "{name} changed after Clauseo's edit.", { name: `\u2068${name}\u2069` });

const button = (name: string) =>
  [...document.querySelectorAll("button")].find((b) => b.textContent === name);
/** Undo waits as DESIGN.md has it: aria-disabled, still in the tab order. */
const waits = () => h!.one("undo")?.getAttribute("aria-disabled") === "true";
/** The queries the room sent PreviewUndo, in order. */
const previews = () =>
  h!.transport
    .only("query")
    .map((c) => c.query)
    .filter((q) => typeof q === "object" && "preview_undo" in q);
/** The dialog's own chunk, loaded on the press as on a real reach. */
const preload = () =>
  act(async () => void (await import("@clauseo/components/clauseo/change-note-ask.tsx")));

const QUERY = { preview_undo: { thread: THREAD, turn: TURN } };

test("nothing edited since: an exec-written paper undoes at once, with no ask", async () => {
  h = await mountRoom(previewUndoScript(() => answer([])));
  h.deliver([reset([changed([file("Notice.docx", true)])])]);
  await preload();
  await h.click(h.one("undo"));
  await h.settle();
  expect(h.sent("undo_step")).toHaveLength(1);
  expect(document.body.textContent).not.toContain("changed after");
  expect(previews()).toEqual([QUERY]);
});

test("PreviewUndo lists a paper: the ask names its file name, and Undo anyway sends the undo", async () => {
  // the first path the host returns, by its file name: the wall shows the paper's path as it is
  h = await mountRoom(previewUndoScript(() => answer(["sub/notes.txt", "Other.docx"])));
  h.deliver([reset([changed([file("sub/notes.txt", true), file("Other.docx")])])]);
  await preload();
  await h.click(h.one("undo"));
  await h.settle();
  expect(document.body.textContent).toContain(askTitle("notes.txt"));
  expect(h.sent("undo_step")).toEqual([]);
  await h.click(button("Undo anyway"));
  expect(h.sent("undo_step")).toHaveLength(1);
  expect(h.sent("undo_step")[0]?.files.map((f) => f.choice)).toEqual(["undo", "undo"]);
});

test("PreviewUndo lists a paper and Keep current sends nothing", async () => {
  h = await mountRoom(previewUndoScript(() => answer(["notes.txt"])));
  h.deliver([reset([changed([file("notes.txt")])])]);
  await preload();
  await h.click(h.one("undo"));
  await h.settle();
  await h.click(button("Keep current"));
  expect(h.sent("undo_step")).toEqual([]);
});

test.each([
  [
    "refused",
    { code: "command.unknown_thread", message: "for logs", retryable: false } as HostError,
  ],
  ["failing", new Error("the host is gone")],
])(
  "PreviewUndo %s with the flag set: the ask still shows, about the flagged paper",
  async (_, failure) => {
    h = await mountRoom(
      previewUndoScript(() => {
        throw failure;
      }),
    );
    h.deliver([reset([changed([file("Notice.docx"), file("Reply.docx", true)])])]);
    await preload();
    await h.click(h.one("undo"));
    await h.settle();
    expect(document.body.textContent).toContain(askTitle("Reply.docx"));
    expect(h.sent("undo_step")).toEqual([]);
  },
);

test("PreviewUndo refused with no flag set: Undo sends at once, as with nothing edited", async () => {
  h = await mountRoom(
    previewUndoScript(() => {
      throw { code: "command.invalid", message: "for logs", retryable: false } satisfies HostError;
    }),
  );
  h.deliver([reset([changed([file("Notice.docx")])])]);
  await preload();
  await h.click(h.one("undo"));
  await h.settle();
  expect(h.sent("undo_step")).toHaveLength(1);
  expect(document.body.textContent).not.toContain("changed after");
});

test("the undo isn't sent before PreviewUndo answers; Undo waits meanwhile, and a press asks nothing more", async () => {
  let release: (edited: string[]) => void = () => {};
  const held = new Promise<string[]>((resolve) => (release = resolve));
  h = await mountRoom(previewUndoScript(() => held.then(answer)));
  h.deliver([reset([changed([file("Notice.docx", true)])])]);
  await h.click(h.one("undo"));
  expect(waits()).toBe(true);
  await h.click(h.one("undo"));
  expect(previews()).toEqual([QUERY]);
  expect(h.sent("undo_step")).toEqual([]);
  await act(async () => release([]));
  await h.settle();
  expect(h.sent("undo_step")).toHaveLength(1);
  expect(waits()).toBe(false);
});

test("each press asks PreviewUndo again: files may change between them", async () => {
  h = await mountRoom(previewUndoScript(() => answer(["Notice.docx"])));
  h.deliver([reset([changed([file("Notice.docx")])])]);
  await preload();
  await h.click(h.one("undo"));
  await h.settle();
  await h.click(button("Keep current"));
  await h.click(h.one("undo"));
  await h.settle();
  expect(previews()).toEqual([QUERY, QUERY]);
  expect(h.sent("undo_step")).toEqual([]);
});
