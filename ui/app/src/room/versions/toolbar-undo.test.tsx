// @vitest-environment happy-dom
// The toolbar's Undo walks the turns' versions newest first: it passes a turn whose change can't be undone (no file,
// or undone already) for an older one that can, and with nothing to undo it opens nothing and waits. Cancel in the
// dialog it opened goes back to where the lawyer was: out of Versions when Undo opened it.
import { act } from "react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { THREAD, TURN, uuid } from "../../testing/stream-script.ts";
import type { ViewEntry } from "../../transport/protocol.ts";
import {
  dialog,
  file,
  matterReset,
  MATTER_STREAM,
  mountVersions,
  NOON,
  stepEntry,
  version,
  type VersionsHarness,
} from "./testing.ts";
import { loadVersions } from "./use-versions.ts";

let h: VersionsHarness | null = null;
beforeEach(() => {
  vi.setSystemTime(NOON + 30 * 60_000);
});
afterEach(async () => {
  await h?.unmount();
  h = null;
  vi.useRealTimers();
});

const LATER = uuid(2, 4);
/** Two turns of the conversation: cp2 (TURN) and the newer cp3 (LATER), then the lawyer's edit. */
const VERSIONS: ViewEntry[] = [
  version(1, "import"),
  version(2, "after_turn", { thread: THREAD, turn: TURN }),
  version(3, "after_turn", { thread: THREAD, turn: LATER }),
  version(4, "outside_edits", { files: [file("other.txt")] }),
];

const rowOf = (h: VersionsHarness, checkpoint: string) =>
  h.all("version").find((r) => r.dataset["checkpoint"] === checkpoint);
const undoButton = (h: VersionsHarness) => h.host.querySelector(".room-pair .room-undo");

/** Presses the toolbar's Undo with Versions shut, and lets Versions open on the matter's stream. */
async function pressUndo(h: VersionsHarness, first = true): Promise<void> {
  await h.click(undoButton(h));
  if (!first) return;
  await act(async () => void (await loadVersions()));
  await h.settle();
  await h.deliver(MATTER_STREAM, [matterReset(VERSIONS)]);
}

test("the toolbar's Undo passes the newest turn, which changed no file, for the older turn that did", async () => {
  h = await mountVersions(VERSIONS, [
    stepEntry(5, { files: [file("brief.txt")], checkpoint_after: "cp2" }),
    stepEntry(9, { turn: LATER, files: [], checkpoint_after: "cp3" }),
  ]);
  await pressUndo(h);
  expect(rowOf(h, "cp2")?.getAttribute("aria-current")).toBe("true");
  expect(dialog()?.textContent).toMatch(/^Undo the change Clauseo made at .+ today\?/);
  expect(dialog()?.textContent).not.toContain("Nothing goes back");
  expect(undoButton(h)?.getAttribute("aria-disabled")).toBeNull();
});

test("with nothing to undo, the toolbar's Undo opens no dialog and waits, faint", async () => {
  h = await mountVersions(VERSIONS, [
    stepEntry(5, { files: [file("brief.txt")], checkpoint_after: "cp2", undone: true }),
    stepEntry(9, { turn: LATER, files: [], checkpoint_after: "cp3" }),
  ]);
  await pressUndo(h);
  expect(dialog()).toBeNull();
  expect(h.one("versions")).not.toBeNull();
  expect(undoButton(h)?.getAttribute("aria-disabled")).toBe("true");
  // pressed again, it asks nothing
  await pressUndo(h, false);
  expect(dialog()).toBeNull();
  expect(h.sent("undo_step")).toEqual([]);
});

test("Cancel in the dialog the toolbar's Undo opened along with Versions closes Versions again", async () => {
  h = await mountVersions(VERSIONS, [
    stepEntry(5, { files: [file("brief.txt")], checkpoint_after: "cp2" }),
    stepEntry(9, { turn: LATER, files: [file("memo.docx")], checkpoint_after: "cp3" }),
  ]);
  await pressUndo(h);
  expect(dialog()).not.toBeNull();
  await h.click(h.button("Cancel", dialog()));
  expect(dialog()).toBeNull();
  expect(h.one("versions")).toBeNull();
  expect(h.host.querySelector("nav.room-rail")).not.toBeNull();
});

test("with Versions open, Cancel in the dialog the toolbar's Undo opened goes back to the version the lawyer was at", async () => {
  h = await mountVersions(VERSIONS, [
    stepEntry(5, { files: [file("brief.txt")], checkpoint_after: "cp2" }),
    stepEntry(9, { turn: LATER, files: [file("memo.docx")], checkpoint_after: "cp3" }),
  ]);
  await h.openVersions();
  await h.lookAt("cp1");
  await pressUndo(h, false);
  expect(rowOf(h, "cp3")?.getAttribute("aria-current")).toBe("true");
  await h.click(h.button("Cancel", dialog()));
  expect(h.one("versions")).not.toBeNull();
  expect(rowOf(h, "cp1")?.getAttribute("aria-current")).toBe("true");
});

test("picking the newest version, a turn's, keeps the room at now and offers that turn's Undo alone in the band", async () => {
  // the conversation's later turn is the newest version
  const NEWEST = VERSIONS.slice(0, 3);
  h = await mountVersions(NEWEST, [
    stepEntry(5, { files: [file("brief.txt")], checkpoint_after: "cp2" }),
    stepEntry(9, { turn: LATER, files: [file("memo.docx")], checkpoint_after: "cp3" }),
  ]);
  await h.openVersions();
  expect(document.body.querySelector(".ver-band")).toBeNull();
  await h.lookAt("cp3");
  // still now: the newest row is current, and there is nothing to restore, compare or go back to
  expect(rowOf(h, "cp3")?.getAttribute("aria-current")).toBe("true");
  expect(document.body.querySelector(".ver-line")?.textContent).toBe("Now");
  expect(h.one("version-restore")).toBeNull();
  expect(h.button("Compare with now")).toBeNull();
  expect(h.button("Back to now")).toBeNull();
  await h.click(h.one("version-undo"));
  expect(dialog()?.textContent).toMatch(/^Undo the change Clauseo made at .+ today\?/);
  await h.click(h.one("dialog-confirm"));
  expect(h.sent("undo_step")).toEqual([
    { thread: THREAD, turn: LATER, files: [{ path: "memo.docx", choice: "undo" }] },
  ]);
});
