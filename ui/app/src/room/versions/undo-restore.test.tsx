// @vitest-environment happy-dom
// Taking a restore back: the restored line's Undo shows only once the host has marked the restore it wrote, never for
// a restore that found nothing to write; "Undo this change" on a restore version and the restored line's Undo both
// ask through the restore dialog, which lists what restoring the version before writes now; and the restored line
// goes once the matter moves past it.
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { MATTER, THREAD, TURN, upsert } from "../../testing/stream-script.ts";
import type { RestorePreview, ViewEntry } from "../../transport/protocol.ts";
import {
  answering,
  describedBy,
  dialog,
  file,
  MATTER_STREAM,
  mountVersions,
  NOON,
  restoredMarker,
  stepEntry,
  version,
  type VersionsHarness,
} from "./testing.ts";

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
const STEPS = [stepEntry(5, { files: [file("brief.txt")], checkpoint_after: "cp2" })];
const PREVIEW: RestorePreview = {
  changes: [file("brief.txt")],
  added_since: [],
  kept: [],
  locked: [],
};

const notice = () =>
  [...document.body.querySelectorAll(".ver-notice")].find((n) =>
    n.textContent?.includes("Restored to"),
  );
const restoreCp2 = async (h: VersionsHarness) => {
  await h.lookAt("cp2");
  await h.click(h.one("version-restore"));
  await h.click(h.one("dialog-confirm"));
};

test("a restore the host answers with a version already there, and no marker, offers no Undo", async () => {
  h = await mountVersions(VERSIONS, STEPS, {
    query: answering({ preview_restore: { restore_preview: PREVIEW } }),
    // nothing needed writing: the host answers with the newest version, cp3, and writes no marker
    dispatch: () => ({ version_restored: { checkpoint: "cp3" } }),
  });
  await h.openVersions();
  await restoreCp2(h);
  expect(h.sent("restore_version")).toHaveLength(1);
  expect(notice()).not.toBeUndefined();
  expect(notice()?.querySelector(".ver-restored-undo")).toBeNull();
  // the room moving on doesn't bring it either: cp3 has no marker
  await h.deliver(MATTER_STREAM, [upsert(version(3, "outside_edits", { files: [file("o.txt")] }))]);
  expect(document.body.querySelector(".ver-restored-undo")).toBeNull();
});

test("the restored line's Undo shows once the host's marker for that restore arrives", async () => {
  h = await mountVersions(VERSIONS, STEPS, {
    query: answering({ preview_restore: { restore_preview: PREVIEW } }),
    dispatch: () => ({ version_restored: { checkpoint: "cp4" } }),
  });
  await h.openVersions();
  await restoreCp2(h);
  expect(notice()?.querySelector(".ver-restored-undo")).toBeNull();
  await h.deliver(MATTER_STREAM, [upsert(version(4, "restore"))]);
  expect(notice()?.querySelector(".ver-restored-undo")).toBeNull();
  await h.deliver(MATTER_STREAM, [upsert(restoredMarker(40, "cp2", "cp4"))]);
  expect(notice()?.querySelector(".ver-restored-undo")).not.toBeNull();
});

test("Undo this change on a restore version asks through the restore dialog, with what restoring the version before writes", async () => {
  h = await mountVersions(
    [...VERSIONS, version(4, "restore"), restoredMarker(40, "cp2", "cp4"), version(5, "import")],
    STEPS,
    {
      query: answering({
        preview_restore: {
          restore_preview: { ...PREVIEW, changes: [file("brief.txt"), file("memo.docx")] },
        },
      }),
      dispatch: () => ({ version_restored: { checkpoint: "cp6" } }),
    },
  );
  await h.openVersions();
  await h.lookAt("cp4");
  await h.click(h.one("version-undo"));
  // nothing is restored yet: the dialog lists the preview of cp3, the version before the restore
  expect(h.sent("restore_version")).toEqual([]);
  expect(h.queried("preview_restore")).toEqual([
    { preview_restore: { matter: MATTER, checkpoint: "cp3", scope: "matter" } },
  ]);
  expect(describedBy()).toBe(
    "2 documents will change: brief.txt and memo.docx. You can undo this.",
  );
  await h.click(h.one("dialog-confirm"));
  expect(h.sent("restore_version")).toEqual([
    { matter: MATTER, checkpoint: "cp3", scope: "matter" },
  ]);
  expect(dialog()).toBeNull();
  expect(document.body.textContent).toMatch(/Undid the restore\. The matter stands as it did at/);
});

test("the restored line goes once the matter's stream moves past the restore", async () => {
  h = await mountVersions(VERSIONS, STEPS, {
    query: answering({ preview_restore: { restore_preview: PREVIEW } }),
    dispatch: () => ({ version_restored: { checkpoint: "cp4" } }),
  });
  await h.openVersions();
  await restoreCp2(h);
  // the restore's own version and marker keep it
  await h.deliver(MATTER_STREAM, [
    upsert(version(4, "restore")),
    upsert(restoredMarker(40, "cp2", "cp4")),
  ]);
  expect(notice()).not.toBeUndefined();
  // a later turn's version: the line, and its Undo, go
  await h.deliver(MATTER_STREAM, [upsert(version(5, "outside_edits", { files: [file("x.txt")] }))]);
  expect(notice()).toBeUndefined();
  expect(document.body.querySelector(".ver-restored-undo")).toBeNull();
});
