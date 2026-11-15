// @vitest-environment happy-dom
// Restoring one document: comparing a version with now offers Restore this document for the document in view, asks
// preview_restore and restore_version for that file alone, and the dialog says a copy there now is replaced (kept in
// Versions) or a removed one comes back; one added since has nothing to bring back; a file whose name is now a folder
// is refused in plain words.
import { errorWords } from "@clauseo/components/lib/words-errors.ts";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { MATTER, THREAD, TURN, upsert, uuid } from "../../testing/stream-script.ts";
import type { FileChange, RestorePreview, ViewEntry } from "../../transport/protocol.ts";
import {
  answering,
  describedBy,
  dialog,
  file,
  MATTER_STREAM,
  mountVersions,
  NOON,
  stepEntry,
  version,
  type Script,
  type VersionsHarness,
} from "./testing.ts";
import { restorable } from "./state.ts";

let h: VersionsHarness | null = null;
beforeEach(() => {
  vi.useFakeTimers({ toFake: ["Date"] });
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
const STEPS = [stepEntry(5, { files: [file("brief.txt")], checkpoint_after: "cp2" })];
const one = (path: string): RestorePreview => ({
  changes: [file(path)],
  added_since: [],
  kept: [],
  locked: [],
});
const ONE_FILE = { files: { paths: ["brief.txt"] } };

/** Versions open on cp2, compared with now, the folder differing in `changes`; preview: preview_restore's answer (a
 * function may throw a refusal), by default one that writes the first file. */
async function comparing(
  changes: FileChange[],
  { dispatch, preview }: { dispatch?: Script["dispatch"]; preview?: unknown } = {},
): Promise<VersionsHarness> {
  const harness = await mountVersions(VERSIONS, STEPS, {
    ...(dispatch === undefined ? {} : { dispatch }),
    query: answering({
      compare_versions: { file_changes: changes },
      preview_restore: preview ?? { restore_preview: one(changes[0]?.path ?? "") },
    }),
  });
  await harness.openVersions();
  await harness.lookAt("cp2");
  await harness.click(harness.button("Compare with now"));
  return harness;
}

test("a changed document goes back alone: the dialog says the copy now is replaced and kept, and its Undo stands", async () => {
  h = await comparing([file("brief.txt"), file("notes.docx")], {
    dispatch: () => ({ version_restored: { checkpoint: "cp4" } }),
  });
  await h.click(h.one("version-restore-file"));
  expect(h.queried("preview_restore")).toEqual([
    { preview_restore: { matter: MATTER, checkpoint: "cp2", scope: ONE_FILE } },
  ]);
  expect(dialog()?.textContent).toContain("Restore '\u2068brief.txt\u2069' to how it was at");
  expect(describedBy()).toBe(
    "This replaces '\u2068brief.txt\u2069' as it is now. The copy you have now stays in Versions, so you can undo this.",
  );
  // the other document that differs is no part of it
  expect(dialog()?.textContent).not.toContain("notes.docx");

  await h.click(h.one("dialog-confirm"));
  expect(h.sent("restore_version")).toEqual([
    { matter: MATTER, checkpoint: "cp2", scope: ONE_FILE, thread: THREAD },
  ]);
  await h.deliver(MATTER_STREAM, [
    upsert(version(4, "restore")),
    upsert({
      marker: {
        order: 40,
        event: uuid(40, 9),
        ts: NOON + 4 * 60_000,
        kind: { version_restored: { from: "cp2", checkpoint: "cp4", scope: ONE_FILE } },
      },
    }),
  ]);
  expect(dialog()).toBeNull();
  const done = [...document.body.querySelectorAll(".ver-notice")].find((n) =>
    n.textContent?.includes("Restored"),
  );
  expect(done?.querySelector(".ver-notice-text")?.textContent).toMatch(
    /^Restored \u2068brief\.txt\u2069 to how it was at .+ today\.$/,
  );
  // the restore can be taken back like any other, by restoring the version just before it
  await h.click(done?.querySelector(".ver-restored-undo"));
  expect(h.queried("preview_restore").at(-1)).toEqual({
    preview_restore: { matter: MATTER, checkpoint: "cp3", scope: "matter", undoes: "cp4" },
  });
});

test("a removed document comes back, and one added since has no Restore this document", async () => {
  h = await comparing(
    [file("brief.txt", { change: "deleted" }), file("new.txt", { change: "added" })],
    // writing it adds it back
    {
      preview: {
        restore_preview: { ...one("brief.txt"), changes: [file("brief.txt", { change: "added" })] },
      },
    },
  );
  await h.click(h.one("version-restore-file"));
  expect(describedBy()).toBe(
    "'\u2068brief.txt\u2069' comes back as it was then. You can undo this.",
  );
  await h.click(h.button("Cancel"));
  expect(dialog()).toBeNull();
  await h.click(
    [...document.body.querySelectorAll(".ver-compare-bar button")].find((b) =>
      b.textContent?.includes("new.txt"),
    ),
  );
  expect(document.body.querySelector(".ver-pdf")?.getAttribute("aria-label")).toContain("new.txt");
  expect(h.one("version-restore-file")).toBeNull();
});

test("a file whose name is now a folder is refused in plain words, and nothing is restored", async () => {
  h = await comparing([file("brief.txt")], {
    preview: () => {
      throw { code: "matter.file_folder_clash", message: "clash", retryable: false };
    },
  });
  await h.click(h.one("version-restore-file"));
  expect(dialog()).toBeNull();
  expect(h.sent("restore_version")).toEqual([]);
  const words = errorWords("en", "matter.file_folder_clash");
  expect(words).toBe(
    "Clauseo can't restore this paper: a folder now has its name, or the folder it was in is now a file. Nothing has changed.",
  );
  expect(document.body.textContent).toContain(words);
});

test("the file a compared document's Restore puts back: its path, or its old path when moved", () => {
  expect(restorable(file("a.docx"))).toBe("a.docx");
  expect(restorable(file("a.docx", { change: "deleted" }))).toBe("a.docx");
  expect(restorable(file("a.docx", { change: "added" }))).toBeNull();
  expect(restorable(file("b.docx", { change: { renamed: { from: "a.docx" } } }))).toBe("a.docx");
  expect(restorable(file("a.docx", { change: "copied" }))).toBeNull();
});
