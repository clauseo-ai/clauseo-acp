// @vitest-environment happy-dom
// Versions' dialog meeting a lock, over FakeTransport: Word's stage only when the undo writes one Word document, or the
// restore's preview names one (as locked, or as the only file it writes); any other lock keeps the confirm stage with
// the error map's line and Try again. Behind the Word stage the band says nothing of the lock; behind the confirm stage
// it gives the general line.
import { errorWords } from "@clauseo/components/lib/words-errors.ts";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { THREAD, TURN } from "../../testing/stream-script.ts";
import type { RestorePreview, ViewEntry } from "../../transport/protocol.ts";
import {
  dialog,
  file,
  mountVersions,
  NOON,
  stepEntry,
  version,
  type VersionsHarness,
} from "../versions/testing.ts";

let h: VersionsHarness | null = null;
beforeEach(() => {
  vi.setSystemTime(NOON + 30 * 60_000);
});
afterEach(async () => {
  await h?.unmount();
  h = null;
  vi.useRealTimers();
});

const VERSIONS: ViewEntry[] = [
  version(1, "import"),
  version(2, "after_turn", { thread: THREAD, turn: TURN }),
  version(3, "outside_edits", { files: [file("other.txt")] }),
];
const locked = { code: "matter.file_locked", message: "held", retryable: true };
const alert = () => dialog()?.querySelector('[role="alert"]')?.textContent ?? null;

async function undoRefused(path: string): Promise<VersionsHarness> {
  const room = await mountVersions(
    VERSIONS,
    [stepEntry(5, { files: [file(path)], checkpoint_after: "cp2" })],
    {
      dispatch: () => {
        throw locked;
      },
    },
  );
  await room.openVersions();
  await room.lookAt("cp2");
  await room.click(room.one("version-undo"));
  await room.click(room.one("dialog-confirm"));
  return room;
}

test("an undo of one .docx refused with the lock: Word's stage, naming it, with Try again", async () => {
  h = await undoRefused("drafts/notes.docx");
  expect(h.sent("undo_step")).toHaveLength(1);
  expect(dialog()?.textContent).toContain(
    "Close '\u2068notes.docx\u2069' in Word to undo the change.",
  );
  expect(h.one("dialog-try-again")).not.toBeNull();
});

test("an undo of one .txt refused with the lock: the confirm stage, with the error map's line and Try again", async () => {
  h = await undoRefused("brief.txt");
  expect(dialog()?.textContent).not.toContain("in Word");
  expect(alert()).toBe(errorWords("en", "matter.file_locked"));
  expect(h.one("dialog-try-again")).not.toBeNull();
  expect(h.one("dialog-confirm")).toBeNull();
});

const previewOf = (paths: string[], lockedPaths: string[] = []): RestorePreview => ({
  changes: paths.map((p) => file(p)),
  added_since: [],
  kept: [],
  locked: lockedPaths,
});

async function restoreRefused(preview: RestorePreview): Promise<VersionsHarness> {
  const room = await mountVersions(
    VERSIONS,
    [stepEntry(5, { files: [file("brief.txt")], checkpoint_after: "cp2" })],
    {
      query: (q) =>
        typeof q === "object" && q.preview_restore !== undefined
          ? { restore_preview: preview }
          : undefined,
      dispatch: () => {
        throw locked;
      },
    },
  );
  await room.openVersions();
  await room.lookAt("cp2");
  await room.click(room.one("version-restore"));
  await room.click(room.one("dialog-confirm"));
  return room;
}

for (const [name, preview, word] of [
  [
    "naming a .docx as locked",
    previewOf(["brief.txt", "notes.docx"], ["notes.docx"]),
    "notes.docx",
  ],
  ["writing one .docx and naming none locked", previewOf(["memo.DOCX"]), "memo.DOCX"],
  ["naming a .txt as locked", previewOf(["brief.txt", "notes.docx"], ["brief.txt"]), null],
  [
    "naming two Word documents as locked",
    previewOf(["a.docx", "b.docx"], ["a.docx", "b.docx"]),
    null,
  ],
  ["writing one .txt and naming none locked", previewOf(["brief.txt"]), null],
] as const) {
  test(`a restore refused with the lock, its preview ${name}: ${word === null ? "the error map's line" : "Word's stage"}`, async () => {
    h = await restoreRefused(preview);
    expect(h.sent("restore_version")).toHaveLength(1);
    if (word === null) {
      expect(dialog()?.textContent).not.toContain("in Word");
      expect(alert()).toBe(errorWords("en", "matter.file_locked"));
    } else {
      expect(dialog()?.textContent).toContain(`Close '\u2068${word}\u2069' in Word to restore it.`);
    }
    expect(h.one("dialog-try-again")).not.toBeNull();
  });
}

const band = () =>
  [...document.body.querySelectorAll(".ver-room-band")].map((n) => n.textContent).join(" ");

test("behind the dialog's Word stage the band doesn't give the general lock line", async () => {
  h = await undoRefused("drafts/notes.docx");
  expect(dialog()?.textContent).toContain("in Word to undo the change.");
  expect(band()).not.toBe("");
  expect(band()).not.toContain(errorWords("en", "matter.file_locked"));
});

test("behind the confirm stage's failure the band still gives the general lock line", async () => {
  h = await undoRefused("brief.txt");
  expect(alert()).toBe(errorWords("en", "matter.file_locked"));
  expect(band()).toContain(errorWords("en", "matter.file_locked"));
});
