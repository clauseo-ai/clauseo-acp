// @vitest-environment happy-dom
// A one-document done line waits for the restore's record: the host answers before the matter's
// stream brings the new version and its marker (the app applies updates once a frame), so until
// then no line is said, never a false "Nothing was restored" or "Nothing was undone"; and a marker
// about another file is no marker.
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { THREAD, TURN, upsert, uuid } from "../../testing/stream-script.ts";
import type { RestorePreview, ViewEntry } from "../../transport/protocol.ts";
import {
  answering,
  file,
  MATTER_STREAM,
  mountVersions,
  NOON,
  stepEntry,
  version,
  type VersionsHarness,
} from "./testing.ts";
import { freshSnapshot } from "./state.ts";
import { doneLine } from "./words.ts";

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
const ONE_FILE = { files: { paths: ["brief.txt"] } };
const preview: RestorePreview = {
  changes: [file("brief.txt")],
  added_since: [],
  kept: [],
  locked: [],
};
/** The marker of a one-document restore at order n, listing what it wrote. */
const marker = (n: number, kind: { from: string; checkpoint: string; undoes?: string }) =>
  upsert({
    marker: {
      order: n * 10,
      event: uuid(n * 10, 9),
      ts: NOON + n * 60_000,
      kind: { version_restored: { scope: ONE_FILE, written: ["brief.txt"], ...kind } },
    },
  });
const lines = () =>
  [...document.body.querySelectorAll(".ver-notice-text")].map((n) => n.textContent).join("\n");
const NOTHING = /Nothing was (restored|undone)/;

test("a Restore and its Undo, answered before the stream brings their record, say no line until it lands", async () => {
  h = await mountVersions(VERSIONS, STEPS, {
    dispatch: (c) => ({
      version_restored: { checkpoint: c.restore_version?.undoes === undefined ? "cp4" : "cp5" },
    }),
    query: answering({
      compare_versions: { file_changes: [file("brief.txt")] },
      preview_restore: { restore_preview: preview },
    }),
  });
  await h.openVersions();
  await h.lookAt("cp2");
  await h.click(h.button("Compare with now"));
  await h.click(h.one("version-restore-file"));
  await h.click(h.one("dialog-confirm"));
  // answered, the update not yet applied: no done line at all, and nothing said to be not done
  expect(h.sent("restore_version")).toHaveLength(1);
  expect(lines()).not.toMatch(NOTHING);
  expect(document.body.textContent).not.toMatch(NOTHING);
  expect(lines()).not.toContain("brief.txt");
  await h.deliver(MATTER_STREAM, [
    upsert(version(4, "restore")),
    marker(4, { from: "cp2", checkpoint: "cp4" }),
  ]);
  expect(lines()).toMatch(/^Restored \u2068brief\.txt\u2069 to how it was at .+ today\.$/m);

  await h.click(document.body.querySelector(".ver-restored-undo"));
  await h.click(h.one("dialog-confirm"));
  expect(h.sent("restore_version")).toHaveLength(2);
  expect(document.body.textContent).not.toMatch(NOTHING);
  expect(lines()).not.toContain("brief.txt");
  await h.deliver(MATTER_STREAM, [
    upsert(version(5, "restore")),
    marker(5, { from: "cp3", checkpoint: "cp5", undoes: "cp4" }),
  ]);
  expect(lines()).toContain("Undid the restore. '\u2068brief.txt\u2069' is back as it was before.");
  expect(lines()).not.toMatch(NOTHING);
});

test("a marker about another file is no marker: the line says nothing was done to this one", () => {
  const other = { file: "notes.docx", wrote: true, removed: false, forGood: false };
  const s = { ...freshSnapshot(NOON, "UTC"), oneFile: new Map([["cp9", other]]) };
  const cal = { today: "2026-11-02", words: () => "" } as never;
  const outcome = { kind: "wrote", checkpoint: "cp9" } as const;
  expect(
    doneLine("en", cal, s, { kind: "restoredFile", to: "cp2", file: "brief.txt", outcome }),
  ).toBe(
    "Nothing was restored: Clauseo couldn't safely bring back that version of '\u2068brief.txt\u2069', so it left it as it is.",
  );
  expect(
    doneLine("en", cal, s, {
      kind: "undidRestore",
      at: NOON,
      file: "brief.txt",
      checkpoint: "cp9",
    }),
  ).toBe(
    "Nothing was undone: Clauseo couldn't safely put '\u2068brief.txt\u2069' back, so it left it as it is.",
  );
});
