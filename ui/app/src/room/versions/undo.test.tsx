// @vitest-environment happy-dom
// Undoing one turn's change: a file that may hold the lawyer's own edits is kept unless the lawyer says otherwise, an
// undo always puts at least one file back, and the toolbar's Undo opens the dialog for the newest change.
import { act } from "react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { STREAM, THREAD, TURN, upsert } from "../../testing/stream-script.ts";
import type { ViewEntry } from "../../transport/protocol.ts";
import {
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
// a fixed instant half an hour after the fixtures' NOON: the rows' "today" depends on it
const NOW = NOON + 30 * 60_000;
beforeEach(() => {
  vi.useFakeTimers({ toFake: ["Date"] });
  vi.setSystemTime(NOW);
});
afterEach(async () => {
  await h?.unmount();
  h = null;
  vi.useRealTimers();
});

/** The import, the turn's version (cp2) and an edit of another file after it, so cp2 isn't now. */
const VERSIONS: ViewEntry[] = [
  version(1, "import"),
  version(2, "after_turn", { thread: THREAD, turn: TURN }),
  version(3, "outside_edits", { files: [file("other.txt")] }),
];

const rowOf = (h: VersionsHarness, checkpoint: string) =>
  h.all("version").find((r) => r.dataset["checkpoint"] === checkpoint);
const dialog = () => document.body.querySelector<HTMLElement>('[role="dialog"]');
const choiceRow = (name: string) =>
  [...(dialog()?.querySelectorAll<HTMLElement>(".ver-edited-row") ?? [])].find((r) =>
    r.querySelector(".ver-edited-name")?.textContent?.includes(name),
  );
const pressed = (row: HTMLElement | undefined, label: string) =>
  [...(row?.querySelectorAll("button") ?? [])]
    .find((b) => b.textContent?.trim() === label)
    ?.getAttribute("aria-pressed");

test("Keep current is chosen for a file that may hold the lawyer's edits, and the undo sends each file's choice", async () => {
  h = await mountVersions(VERSIONS, [
    stepEntry(5, {
      files: [file("a.docx", { may_include_lawyer_edits: true }), file("b.docx")],
      checkpoint_after: "cp2",
    }),
  ]);
  await h.openVersions();
  await h.click(rowOf(h, "cp2"));
  await h.click(h.one("version-undo"));
  expect(dialog()).not.toBeNull();
  // only the file that may hold the lawyer's edits asks; b goes back without asking
  expect(choiceRow("b.docx")).toBeUndefined();
  const a = choiceRow("a.docx");
  expect(pressed(a, "Keep current")).toBe("true");
  expect(pressed(a, "Undo anyway")).toBe("false");
  // the row says more than the heading: what the watcher saw, not a guess repeated
  expect(dialog()?.querySelector(".ver-edited-head")?.textContent).toBe(
    "1 document may hold your own edits.",
  );
  expect(a?.querySelector(".ver-edited-at")?.textContent).toBe(
    "Changed during the answer, though Clauseo didn't say it was changing it.",
  );
  expect(h.one("dialog-confirm")?.getAttribute("aria-disabled")).toBeNull();
  await h.click(h.one("dialog-confirm"));
  expect(h.sent("undo_step")).toEqual([
    {
      thread: THREAD,
      turn: TURN,
      files: [
        { path: "a.docx", choice: "keep" },
        { path: "b.docx", choice: "undo" },
      ],
    },
  ]);
  expect(dialog()).toBeNull();
});

test("an undo always puts a file back: with its only file kept it waits, and Undo anyway lets it go ahead", async () => {
  h = await mountVersions(VERSIONS, [
    stepEntry(5, {
      files: [file("a.docx", { may_include_lawyer_edits: true })],
      checkpoint_after: "cp2",
    }),
  ]);
  await h.openVersions();
  await h.click(rowOf(h, "cp2"));
  await h.click(h.one("version-undo"));
  const confirm = h.one("dialog-confirm");
  expect(confirm?.getAttribute("aria-disabled")).toBe("true");
  expect(dialog()?.textContent).toContain(
    "Nothing goes back. Every document it changed is kept as it is now.",
  );
  await h.click(confirm);
  expect(h.sent("undo_step")).toEqual([]);
  expect(dialog()).not.toBeNull();

  await h.click(h.button("Undo anyway", choiceRow("a.docx")));
  expect(pressed(choiceRow("a.docx"), "Undo anyway")).toBe("true");
  expect(h.one("dialog-confirm")?.getAttribute("aria-disabled")).toBeNull();
  await h.click(h.one("dialog-confirm"));
  expect(h.sent("undo_step")).toEqual([
    { thread: THREAD, turn: TURN, files: [{ path: "a.docx", choice: "undo" }] },
  ]);
});

test("the toolbar's Undo opens Versions on the newest turn's change, with its undo dialog", async () => {
  h = await mountVersions(
    VERSIONS,
    [stepEntry(5, { files: [file("brief.txt")], checkpoint_after: "cp2" })],
    { dispatch: () => ({ step_undone: { checkpoint: "cp4" } }) },
  );
  expect(h.one("versions")).toBeNull();
  await h.click(h.host.querySelector(".room-pair .room-undo"));
  await act(async () => void (await loadVersions()));
  await h.settle();
  await h.deliver(MATTER_STREAM, [matterReset(VERSIONS)]);
  expect(h.one("versions")).not.toBeNull();
  // the room looks at that change, and the dialog asks to undo it
  expect(rowOf(h, "cp2")?.getAttribute("aria-current")).toBe("true");
  expect(dialog()?.textContent).toMatch(/^Undo the change Clauseo made at .+ today\?/);
  await h.click(h.one("dialog-confirm"));
  expect(h.sent("undo_step")).toEqual([
    { thread: THREAD, turn: TURN, files: [{ path: "brief.txt", choice: "undo" }] },
  ]);
  // done: back at now, saying what went back
  expect(document.body.textContent).toMatch(
    /Undid the change at .+\. \u2068brief\.txt\u2069 went back/,
  );
});

test("the toolbar's Undo waiting for its step is dropped once the lawyer picks another version", async () => {
  // the newest turn's step isn't in the conversation's window yet
  h = await mountVersions(VERSIONS, []);
  await h.click(h.host.querySelector(".room-pair .room-undo"));
  await act(async () => void (await loadVersions()));
  await h.settle();
  await h.deliver(MATTER_STREAM, [matterReset(VERSIONS)]);
  expect(rowOf(h, "cp2")?.getAttribute("aria-current")).toBe("true");
  expect(dialog()).toBeNull();
  // the lawyer moves on to the import
  await h.click(rowOf(h, "cp1"));
  // the step comes in late: no dialog opens, and the room stays where the lawyer looks
  await h.deliver(STREAM, [
    upsert(stepEntry(5, { files: [file("brief.txt")], checkpoint_after: "cp2" })),
  ]);
  expect(dialog()).toBeNull();
  expect(rowOf(h, "cp1")?.getAttribute("aria-current")).toBe("true");
});
