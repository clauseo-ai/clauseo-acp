// @vitest-environment happy-dom
// Restore when the host refuses its preview (preview_restore rejects): the page says why by the code's line, never the
// host's message; a file a program holds opens Word's dialog or the lock dialog; and a second press asks again.
import { errorWords } from "@clauseo/components/lib/words-errors.ts";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { THREAD, TURN } from "../../testing/stream-script.ts";
import type { HostQuery, RestorePreview, ViewEntry } from "../../transport/protocol.ts";
import {
  dialog,
  file,
  mountVersions,
  NOON,
  stepEntry,
  version,
  type VersionsHarness,
} from "./testing.ts";

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
  vi.restoreAllMocks();
});

const VERSIONS: ViewEntry[] = [
  version(1, "import"),
  version(2, "after_turn", { thread: THREAD, turn: TURN }),
  version(3, "outside_edits", { files: [file("other.txt")] }),
];
const STEPS = [stepEntry(5, { files: [file("brief.txt")], checkpoint_after: "cp2" })];
const PREVIEW: RestorePreview = {
  changes: [file("brief.txt")],
  added_since: [],
  kept: [],
  locked: [],
};

const isPreview = (q: HostQuery): boolean =>
  typeof q === "object" && q.preview_restore !== undefined;
// the host's message names the path: it must never reach the page
const refused = (code: string, over: { files?: string[] } = {}) =>
  Promise.reject({
    code,
    message: "C:\\Matters\\brief.txt: sharing violation",
    retryable: true,
    ...over,
  });

const press = async (h: VersionsHarness) => {
  await h.lookAt("cp2");
  await h.click(h.one("version-restore"));
};

test("a refused preview says the code's line on the page, never the host's message, and opens no dialog", async () => {
  const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
  h = await mountVersions(VERSIONS, STEPS, {
    query: (q) => (isPreview(q as HostQuery) ? refused("store.io") : undefined),
  });
  await h.openVersions();
  await press(h);
  expect(dialog()).toBeNull();
  expect(document.body.textContent).toContain(errorWords("en", "store.io"));
  expect(document.body.textContent).not.toContain("sharing violation");
  expect(warn).toHaveBeenCalledWith(expect.stringContaining("preview_restore failed"));
});

test("a refused preview with no files named says the code's line and opens no dialog", async () => {
  h = await mountVersions(VERSIONS, STEPS, {
    query: (q) => (isPreview(q as HostQuery) ? refused("matter.file_locked") : undefined),
  });
  await h.openVersions();
  await press(h);
  expect(dialog()).toBeNull();
  expect(document.body.textContent).toContain(errorWords("en", "matter.file_locked"));
});

test("a second press after a refused preview asks again, and the dialog opens once it answers", async () => {
  vi.spyOn(console, "warn").mockImplementation(() => {});
  let refusing = true;
  h = await mountVersions(VERSIONS, STEPS, {
    query: (q) =>
      isPreview(q as HostQuery)
        ? refusing
          ? refused("store.io")
          : { restore_preview: PREVIEW }
        : undefined,
  });
  await h.openVersions();
  await press(h);
  expect(h.queried("preview_restore")).toHaveLength(1);
  expect(dialog()).toBeNull();

  refusing = false;
  await h.click(h.one("version-restore"));
  expect(h.queried("preview_restore")).toHaveLength(2);
  expect(dialog()).not.toBeNull();
  expect(document.body.textContent).not.toContain(errorWords("en", "store.io"));
});

test("a refused preview that names a Word document a program holds opens Word's dialog for it", async () => {
  vi.spyOn(console, "warn").mockImplementation(() => {});
  h = await mountVersions(VERSIONS, STEPS, {
    query: (q) =>
      isPreview(q as HostQuery)
        ? refused("matter.file_locked", { files: ["notes.docx"] })
        : undefined,
  });
  await h.openVersions();
  await press(h);
  expect(dialog()?.textContent).toContain("Close '\u2068notes.docx\u2069' in Word to restore it.");
  expect(h.one("dialog-try-again")).not.toBeNull();
  expect(h.one("dialog-confirm")).toBeNull();
});

test("a refused preview naming a file a program holds, not Word's, opens the restore dialog", async () => {
  vi.spyOn(console, "warn").mockImplementation(() => {});
  let asks = 0;
  h = await mountVersions(VERSIONS, STEPS, {
    // refused once; the lock dialog asks again, and that one answers with what restoring writes
    query: (q) =>
      isPreview(q as HostQuery)
        ? (asks += 1) === 1
          ? refused("matter.file_locked", { files: ["brief.txt"] })
          : { restore_preview: PREVIEW }
        : undefined,
  });
  await h.openVersions();
  await press(h);
  expect(h.queried("preview_restore")).toHaveLength(2);
  expect(dialog()?.textContent).toContain(errorWords("en", "matter.file_locked"));
  expect(h.one("dialog-try-again")).not.toBeNull();
});
