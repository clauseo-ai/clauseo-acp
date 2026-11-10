// @vitest-environment happy-dom
// A restore made from Versions names the conversation the lawyer is in, so the host records the restore there too
// (the thread's quiet line is restored-row.test.tsx's).
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { MATTER, THREAD, TURN } from "../../testing/stream-script.ts";
import type { ViewEntry } from "../../transport/protocol.ts";
import { answering, file, mountVersions, NOON, version, type VersionsHarness } from "./testing.ts";

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

test("Restore this version sends the open conversation's thread, which records the restore", async () => {
  h = await mountVersions(VERSIONS, [], {
    query: answering({
      preview_restore: {
        restore_preview: { changes: [file("other.txt")], added_since: [], kept: [], locked: [] },
      },
    }),
    dispatch: () => ({ version_restored: { checkpoint: "cp4" } }),
  });
  await h.openVersions();
  await h.lookAt("cp2");
  await h.click(h.one("version-restore"));
  await h.click(h.one("dialog-confirm"));
  expect(h.sent("restore_version")).toEqual([
    { matter: MATTER, checkpoint: "cp2", scope: "matter", thread: THREAD },
  ]);
});
