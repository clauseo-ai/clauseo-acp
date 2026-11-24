// @vitest-environment happy-dom
// A paper another conversation's answer also changed (15a's FileChange.also_changed_by, parallel answers): the
// changed-paper card says so; the thread's Undo opens Versions' undo dialog whatever PreviewUndo says, the paper's row
// says undoing takes the other conversation's change away too, and it is kept (unticked) unless the lawyer says
// otherwise, as a paper that may hold the lawyer's own edits is. Once undone, the card's line goes.
import { act } from "react";
import { vt } from "@clauseo/components/lib/words-versions.ts";
import { afterEach, expect, test } from "vitest";
import { reset, step, uuid } from "../../testing/stream-script.ts";
import type { FileChange, ViewEntry } from "../../transport/protocol.ts";
import { loadVersions } from "../versions/use-versions.ts";
import { mountRoom, previewUndoScript, type Harness } from "./harness.ts";

let h: Harness | null = null;
afterEach(async () => {
  await h?.unmount();
  h = null;
  document.body.replaceChildren();
});

const OTHER = uuid(7, 1);
const paper = (path: string, also: boolean): FileChange => ({
  path,
  change: "modified",
  size_bytes: 10,
  may_include_lawyer_edits: false,
  ...(also ? { also_changed_by: [OTHER] } : {}),
});
const changed = (files: FileChange[], undone = false): ViewEntry =>
  step(9, { files, checkpoint_after: uuid(1, 9), undone });
const nothingSince = () => ({ undo_preview: { edited_since: [] } });
const preload = () => act(async () => void (await loadVersions()));
const confirm = () => document.querySelector<HTMLElement>('[data-testid="dialog-confirm"]');
const choice = (name: string) =>
  [...document.querySelectorAll<HTMLElement>(".ver-edited-row button")].find(
    (b) => b.textContent?.trim() === name,
  );
const ROW = "Another conversation also changed this paper; undoing takes its change away too.";

test("the card says another conversation also changed the paper, naming it", async () => {
  h = await mountRoom(previewUndoScript(nothingSince));
  h.deliver([reset([changed([paper("Notice.docx", true), paper("Reply.docx", false)])])]);
  await h.settle();
  expect(h.one("also-changed")?.textContent).toBe(
    vt(
      "en",
      "Another conversation also changed {papers}. Undoing this takes its change away too.",
      {
        papers: "Notice.docx",
      },
    ),
  );
});

test("no paper shared, no line; and none once the change is undone", async () => {
  h = await mountRoom(previewUndoScript(nothingSince));
  h.deliver([reset([changed([paper("Reply.docx", false)])])]);
  await h.settle();
  expect(h.one("also-changed")).toBeNull();
  await h.unmount();
  h = await mountRoom(previewUndoScript(nothingSince));
  h.deliver([reset([changed([paper("Notice.docx", true)], true)])]);
  await h.settle();
  expect(h.one("also-changed")).toBeNull();
});

test("Undo asks: the shared paper's row says so and is kept, the other goes back", async () => {
  h = await mountRoom(previewUndoScript(nothingSince));
  h.deliver([reset([changed([paper("Notice.docx", true), paper("Reply.docx", false)])])]);
  await preload();
  await h.click(h.one("undo"));
  await h.settle();
  expect(h.sent("undo_step")).toEqual([]);
  expect(document.body.textContent).toContain(vt("en", ROW));
  // not the lawyer's-own-edits head: that would be untrue here
  expect(document.body.textContent).not.toContain(vt("en", "1 document may hold your own edits."));
  expect(choice("Keep current")?.getAttribute("aria-pressed")).toBe("true");
  await h.click(confirm());
  await h.settle();
  expect(h.sent("undo_step")[0]?.files).toEqual([
    { path: "Notice.docx", choice: "keep" },
    { path: "Reply.docx", choice: "undo" },
  ]);
});
