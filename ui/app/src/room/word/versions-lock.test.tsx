// @vitest-environment happy-dom
// Versions' dialog meeting a lock, over FakeTransport: Word's stage only when the files the host names as held (or, for
// an older host, the files the undo writes, or the restore's preview names as locked) are all Word documents: one
// document's line, or two or more named in one line. Any other lock keeps the confirm stage with the error map's line
// and Try again. Behind the Word stage the band says nothing of the lock; behind the confirm stage it gives the general
// line.
import { errorWords } from "@clauseo/components/lib/words-errors.ts";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { THREAD, TURN } from "../../testing/stream-script.ts";
import type { HostError, RestorePreview, ViewEntry } from "../../transport/protocol.ts";
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
const locked: HostError = { code: "matter.file_locked", message: "held", retryable: true };
const alert = () => dialog()?.querySelector('[role="alert"]')?.textContent ?? null;
/** A name as the line shows it: isolated, between the single quotes. */
const named = (path: string) => `'\u2068${path}\u2069'`;

const withFiles = (files: string[]): HostError => ({ ...locked, files });

async function undoRefused(paths: string[], error: HostError = locked): Promise<VersionsHarness> {
  const room = await mountVersions(
    VERSIONS,
    [stepEntry(5, { files: paths.map((p) => file(p)), checkpoint_after: "cp2" })],
    {
      dispatch: () => {
        throw error;
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
  h = await undoRefused(["drafts/notes.docx"]);
  expect(h.sent("undo_step")).toHaveLength(1);
  expect(dialog()?.textContent).toContain(
    "Close '\u2068notes.docx\u2069' in Word to undo the change.",
  );
  expect(h.one("dialog-try-again")).not.toBeNull();
});

test("an undo of three files refused naming one .docx: Word's stage names only that one", async () => {
  h = await undoRefused(["a/Lease.docx", "b/Notice.docx", "c/brief.txt"], {
    ...locked,
    files: ["a/Lease.docx"],
  });
  expect(dialog()?.textContent).toContain(
    `Close ${named("Lease.docx")} in Word to undo the change.`,
  );
  expect(dialog()?.textContent).not.toContain("Notice.docx");
  expect(h.one("dialog-try-again")).not.toBeNull();
});

test("an undo refused naming two Word documents: Word's stage names both, in one line", async () => {
  h = await undoRefused(["a/Lease.docx", "b/Notice.docx"], {
    ...locked,
    files: ["a/Lease.docx", "b/Notice.docx"],
  });
  expect(dialog()?.textContent).toContain(
    `Close ${named("Lease.docx")} and ${named("Notice.docx")} in Word to undo the change.`,
  );
  expect(h.one("dialog-try-again")).not.toBeNull();
});

test("an undo of one .txt refused with the lock: the confirm stage, with the error map's line and Try again", async () => {
  h = await undoRefused(["brief.txt"]);
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

async function restoreRefused(
  preview: RestorePreview,
  error: HostError = locked,
): Promise<VersionsHarness> {
  const room = await mountVersions(
    VERSIONS,
    [stepEntry(5, { files: [file("brief.txt")], checkpoint_after: "cp2" })],
    {
      query: (q) =>
        typeof q === "object" && q.preview_restore !== undefined
          ? { restore_preview: preview }
          : undefined,
      dispatch: () => {
        throw error;
      },
    },
  );
  await room.openVersions();
  await room.lookAt("cp2");
  await room.click(room.one("version-restore"));
  await room.click(room.one("dialog-confirm"));
  return room;
}

// the Word documents the restore's line names, or null for the error map's line
for (const [name, preview, error, words] of [
  [
    "naming a .docx as locked",
    previewOf(["brief.txt", "notes.docx"], ["notes.docx"]),
    locked,
    ["notes.docx"],
  ],
  ["writing one .docx and naming none locked", previewOf(["memo.DOCX"]), locked, ["memo.DOCX"]],
  ["naming a .txt as locked", previewOf(["brief.txt", "notes.docx"], ["brief.txt"]), locked, null],
  [
    "naming two Word documents as locked",
    previewOf(["a.docx", "b.docx"], ["a.docx", "b.docx"]),
    locked,
    ["a.docx", "b.docx"],
  ],
  [
    "naming two Word documents as locked, the host naming one .txt",
    previewOf(["a.docx", "b.docx"], ["a.docx", "b.docx"]),
    withFiles(["brief.txt"]),
    null,
  ],
  [
    "writing a .docx and a .txt, the host naming the .docx as held",
    previewOf(["brief.txt", "notes.docx"]),
    withFiles(["notes.docx"]),
    ["notes.docx"],
  ],
  ["writing one .txt and naming none locked", previewOf(["brief.txt"]), locked, null],
] as const) {
  test(`a restore refused with the lock, its preview ${name}: ${words === null ? "the error map's line" : "Word's stage"}`, async () => {
    h = await restoreRefused(preview, error);
    expect(h.sent("restore_version")).toHaveLength(1);
    if (words === null) {
      expect(dialog()?.textContent).not.toContain("in Word");
      expect(alert()).toBe(errorWords("en", "matter.file_locked"));
    } else if (words.length === 1) {
      expect(dialog()?.textContent).toContain(
        `Close ${named(words[0] ?? "")} in Word to restore it.`,
      );
    } else {
      expect(dialog()?.textContent).toContain(
        `Close ${words.map(named).join(" and ")} in Word to restore it.`,
      );
    }
    expect(h.one("dialog-try-again")).not.toBeNull();
  });
}

const band = () =>
  [...document.body.querySelectorAll(".ver-room-band")].map((n) => n.textContent).join(" ");

test("behind the dialog's Word stage the band doesn't give the general lock line", async () => {
  h = await undoRefused(["drafts/notes.docx"]);
  expect(dialog()?.textContent).toContain("in Word to undo the change.");
  expect(band()).not.toBe("");
  expect(band()).not.toContain(errorWords("en", "matter.file_locked"));
});

test("behind the confirm stage's failure the band still gives the general lock line", async () => {
  h = await undoRefused(["brief.txt"]);
  expect(alert()).toBe(errorWords("en", "matter.file_locked"));
  expect(band()).toContain(errorWords("en", "matter.file_locked"));
});
