// @vitest-environment happy-dom
// The thread's Undo for a step with a file that may hold the lawyer's own edits (the release dry run's "Lawyer
// note.txt": saved into the matter during a read-only answer, deleted by an Undo that never asked): whatever
// PreviewUndo says, the press opens Versions' undo dialog, which says the file may hold the lawyer's own edits and
// keeps it unless the lawyer chooses otherwise; Undo sends "keep" for it. A step with no such file and nothing changed
// since still undoes at once.
import { act } from "react";
import { settled } from "@clauseo/components/lib/late.ts";
import { vt } from "@clauseo/components/lib/words-versions.ts";
import { afterEach, expect, test, vi } from "vitest";
import { reset, step, uuid } from "../../testing/stream-script.ts";
import type { FileChange, HostError, ViewEntry } from "../../transport/protocol.ts";
import { loadVersions } from "../versions/use-versions.ts";
import { defaultOutcome, mountRoom, previewUndoScript, type Harness } from "./harness.ts";

let h: Harness | null = null;
afterEach(async () => {
  await h?.unmount();
  h = null;
  document.body.replaceChildren();
  vi.useRealTimers();
});

const NOTE = "Lawyer note.txt";

/** A file the step added; flagged: no tool call of Clauseo's wrote it. */
const added = (path: string, flagged = false): FileChange => ({
  path,
  change: "added",
  size_bytes: 10,
  may_include_lawyer_edits: flagged,
});

/** A file the step modified, flagged: a file that was there before, edited by the lawyer during the answer. */
const modifiedFlagged = (path: string): FileChange => ({
  path,
  change: "modified",
  size_bytes: 10,
  may_include_lawyer_edits: true,
});

const changed = (files: FileChange[]): ViewEntry =>
  step(9, { files, checkpoint_after: uuid(1, 9) });

/** PreviewUndo's answer: nothing changed since the step. */
const nothingSince = () => ({ undo_preview: { edited_since: [] } });

/** Versions' chunk, which holds the dialog, loaded as on a real press. */
const preload = () => act(async () => void (await loadVersions()));

/** The dialog is in a portal, outside the room's own node. */
const confirm = () => document.querySelector<HTMLElement>('[data-testid="dialog-confirm"]');
/** A choice of the dialog's row, by its words (a toggle button, pressed when chosen). */
const choice = (name: string) =>
  [...document.querySelectorAll<HTMLElement>(".ver-edited-row button")].find(
    (b) => b.textContent?.trim() === name,
  );

test("a flagged file with nothing changed since: the dialog asks, keeps it, and Undo sends keep for it", async () => {
  h = await mountRoom(previewUndoScript(nothingSince));
  h.deliver([reset([changed([added(NOTE, true), added("agent.txt")])])]);
  await preload();
  await h.click(h.one("undo"));
  await h.settle();
  // nothing went out on the press: the dialog asks first
  expect(h.sent("undo_step")).toEqual([]);
  expect(document.body.textContent).toContain(vt("en", "1 document may hold your own edits."));
  expect(document.body.textContent).toContain(
    vt("en", "Changed during the answer, though Clauseo didn't say it was changing it."),
  );
  // Keep current is the note's choice until the lawyer says otherwise
  expect(choice("Keep current")?.getAttribute("aria-pressed")).toBe("true");
  await h.click(confirm());
  await h.settle();
  expect(h.sent("undo_step")).toHaveLength(1);
  expect(h.sent("undo_step")[0]?.files).toEqual([
    { path: NOTE, choice: "keep" },
    { path: "agent.txt", choice: "undo" },
  ]);
});

test("a flagged modified file with nothing changed since: the dialog asks, keeps it, and Undo sends keep for it", async () => {
  h = await mountRoom(previewUndoScript(nothingSince));
  h.deliver([reset([changed([modifiedFlagged("Brief.docx"), added("agent.txt")])])]);
  await preload();
  await h.click(h.one("undo"));
  await h.settle();
  // nothing went out on the press: the dialog asks first
  expect(h.sent("undo_step")).toEqual([]);
  expect(document.body.textContent).toContain(vt("en", "1 document may hold your own edits."));
  expect(choice("Keep current")?.getAttribute("aria-pressed")).toBe("true");
  await h.click(confirm());
  await h.settle();
  expect(h.sent("undo_step")[0]?.files).toEqual([
    { path: "Brief.docx", choice: "keep" },
    { path: "agent.txt", choice: "undo" },
  ]);
});

test("a step whose only file is flagged: Undo waits while it is kept, and goes once the lawyer chooses Undo anyway", async () => {
  h = await mountRoom(previewUndoScript(nothingSince));
  h.deliver([reset([changed([added(NOTE, true)])])]);
  await preload();
  await h.click(h.one("undo"));
  await h.settle();
  expect(confirm()?.getAttribute("aria-disabled")).toBe("true");
  await h.click(confirm());
  expect(h.sent("undo_step")).toEqual([]);
  await h.click(choice("Undo anyway"));
  await h.click(confirm());
  await h.settle();
  expect(h.sent("undo_step")[0]?.files).toEqual([{ path: NOTE, choice: "undo" }]);
});

test("no file flagged and nothing changed since: Undo goes at once, every file undone", async () => {
  h = await mountRoom(previewUndoScript(nothingSince));
  h.deliver([reset([changed([added("agent.txt")])])]);
  await preload();
  await h.click(h.one("undo"));
  await h.settle();
  expect(confirm()).toBeNull();
  expect(h.sent("undo_step")[0]?.files).toEqual([{ path: "agent.txt", choice: "undo" }]);
});

test("Word holding the other file: Try again sends the lawyer's choices again, the note still kept", async () => {
  const locked: HostError = { code: "matter.file_locked", message: "held", retryable: true };
  let held = true;
  h = await mountRoom({
    ...previewUndoScript(nothingSince),
    dispatch: (command) => {
      if (command.undo_step !== undefined && held) throw locked;
      return defaultOutcome(command);
    },
  });
  h.deliver([reset([changed([added(NOTE, true), added("Reply.docx")])])]);
  await preload();
  await h.click(h.one("undo"));
  await h.settle();
  await h.click(confirm());
  await act(settled);
  await h.settle();
  held = false;
  await h.click(h.one("word-lock-retry"));
  await h.settle();
  const kept = [
    { path: NOTE, choice: "keep" },
    { path: "Reply.docx", choice: "undo" },
  ];
  expect(h.sent("undo_step").map((u) => u.files)).toEqual([kept, kept]);
});
