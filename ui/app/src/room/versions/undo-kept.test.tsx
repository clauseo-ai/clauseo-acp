// @vitest-environment happy-dom
// An undo that keeps a file Clauseo had no earlier copy of (the step_undone marker's unversioned):
// a line for each such file, under the band and under the thread's change note, by why it was kept
// and the folder's sync service; and neither the band's line nor the note's count names a kept
// file as gone back.
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { STREAM, THREAD, TURN, upsert, uuid } from "../../testing/stream-script.ts";
import type { FolderPlace, SkippedFile, ViewEntry } from "../../transport/protocol.ts";
import { detailed, fakeNext } from "../fake-next.ts";
import {
  file,
  MATTER_STREAM,
  mountVersions,
  NOON,
  stepEntry,
  THE_MATTER,
  version,
  type VersionsHarness,
} from "./testing.ts";
import { cantUndoWords } from "./words.ts";

let h: VersionsHarness | null = null;
beforeEach(() => {
  vi.setSystemTime(NOON + 30 * 60_000);
});
afterEach(async () => {
  await h?.unmount();
  h = null;
  vi.useRealTimers();
});

/** The words without the isolates the slots are wrapped in. */
const plain = (s: string | null | undefined) => (s ?? "").replace(/[\u2068\u2069]/g, "");
const ONE_DRIVE: FolderPlace = { synced: { service: "one_drive" } };
const ANNEX = "papers/Annexure 3.pdf";

test("the three lines, in English and Hindi, by why the file was kept and the folder's service", () => {
  const words = (lang: "en" | "hi", reason: SkippedFile["reason"], place?: FolderPlace) =>
    plain(cantUndoWords(lang, ANNEX, reason, place));
  expect(words("en", "cloud_only", ONE_DRIVE)).toBe(
    "Clauseo can't undo the change to Annexure 3.pdf. It was only in OneDrive before, so there's no earlier copy here. OneDrive's version history has the earlier one.",
  );
  // a folder no named service syncs, or one not synced at all, or a service with no name to show
  const unknown =
    "Clauseo can't undo the change to Annexure 3.pdf. It was only online before, so there's no earlier copy here. Your cloud service's version history may have the earlier one.";
  expect(words("en", "cloud_only", "local")).toBe(unknown);
  expect(words("en", "cloud_only")).toBe(unknown);
  expect(words("en", "cloud_only", { synced: { service: "other" } })).toBe(unknown);
  const noCopy =
    "Clauseo can't undo the change to Annexure 3.pdf. It didn't keep a copy of it before, so there's no earlier copy to bring back.";
  expect(words("en", "over_size_cap", ONE_DRIVE)).toBe(noCopy);
  expect(words("en", "unknown", ONE_DRIVE)).toBe(noCopy);
  expect(words("hi", "cloud_only", ONE_DRIVE)).toBe(
    "Clauseo Annexure 3.pdf में हुआ बदलाव पूर्ववत नहीं कर सकता। यह पहले केवल OneDrive में था, इसलिए इसकी कोई पुरानी प्रति यहाँ नहीं है। पुरानी प्रति OneDrive के पुराने संस्करणों में है।",
  );
  expect(words("hi", "cloud_only", "network")).toBe(
    "Clauseo Annexure 3.pdf में हुआ बदलाव पूर्ववत नहीं कर सकता। यह पहले केवल ऑनलाइन था, इसलिए इसकी कोई पुरानी प्रति यहाँ नहीं है। पुरानी प्रति शायद आपकी क्लाउड सेवा के पुराने संस्करणों में हो।",
  );
  expect(words("hi", "over_size_cap")).toBe(
    "Clauseo Annexure 3.pdf में हुआ बदलाव पूर्ववत नहीं कर सकता। इसकी कोई प्रति पहले नहीं रखी गई थी, इसलिए वापस लाने के लिए कोई पुरानी प्रति नहीं है।",
  );
});

const VERSIONS: ViewEntry[] = [
  version(1, "import"),
  version(2, "after_turn", { thread: THREAD, turn: TURN }),
];

/**  The host's undo of the turn: its version, the step undone, and the marker naming what it put
 * back and kept. */
async function hostUndid(
  h: VersionsHarness,
  files: string[],
  restored: string[],
  unversioned: SkippedFile[],
): Promise<void> {
  await h.deliver(MATTER_STREAM, [upsert(version(4, "undo", { thread: THREAD, turn: TURN }))]);
  const kept = unversioned.map((f) => f.path);
  const marker: ViewEntry = {
    marker: {
      order: 50,
      event: uuid(50, 8),
      ts: NOON + 50 * 60_000,
      kind: { step_undone: { turn: TURN, checkpoint: "cp4", restored, kept, unversioned } },
    },
  };
  const step = stepEntry(5, { files: files.map((p) => file(p)), checkpoint_after: "cp2" });
  const undone: ViewEntry = { step: { ...step.step!, undone: true } };
  await h.deliver(STREAM, [upsert(undone), upsert(marker)]);
}

/**  The room over a folder at `place`, its turn having changed `files`, undone from Versions' cp2
 * with every file. */
async function undone(files: string[], place: FolderPlace): Promise<VersionsHarness> {
  const harness = await mountVersions(
    VERSIONS,
    [stepEntry(5, { files: files.map((p) => file(p)), checkpoint_after: "cp2" })],
    {
      dispatch: () => ({ step_undone: { checkpoint: "cp4" } }),
      next: fakeNext({
        matters: () => [detailed(THE_MATTER)],
        parts: ["papers"],
        listing: { place },
      }),
    },
  );
  await harness.openVersions();
  await harness.lookAt("cp2");
  await harness.click(harness.one("version-undo"));
  await harness.click(harness.one("dialog-confirm"));
  return harness;
}

const notices = () =>
  [...document.body.querySelectorAll<HTMLElement>(".ver-notice")].map((n) => ({
    tone: n.dataset["tone"],
    text: plain(n.textContent),
  }));
const noteLines = () => plain(h?.one("files-changed")?.textContent);

test("one kept file, only in OneDrive before: the band names it apart, never as gone back, and so does the note", async () => {
  h = await undone(["brief.txt", ANNEX], ONE_DRIVE);
  await hostUndid(h, ["brief.txt", ANNEX], ["brief.txt"], [{ path: ANNEX, reason: "cloud_only" }]);
  const lines = notices();
  expect(lines[0]?.text).toMatch(
    /^Undid the change at .+\. brief\.txt went back, and the undo is a version of its own\.$/,
  );
  expect(lines[0]?.text).not.toContain("Annexure");
  expect(lines[1]).toEqual({
    tone: "failed",
    text: plain(cantUndoWords("en", ANNEX, "cloud_only", ONE_DRIVE)),
  });
  expect(lines[1]?.text).toContain("OneDrive's version history has the earlier one.");
  // the thread's note counts the one that went back, and says why the other stayed
  await vi.waitFor(() => expect(h?.all("cant-undo")).toHaveLength(1));
  expect(noteLines()).toContain("You undid Clauseo's changes to 1 paper.");
  expect(plain(h.one("cant-undo")?.textContent)).toBe(lines[1]?.text);
}, 30_000);

test("every file kept: no line says a file went back; one line each says why it stays", async () => {
  h = await undone([ANNEX, "Bank statement.pdf"], "local");
  await hostUndid(
    h,
    [ANNEX, "Bank statement.pdf"],
    [],
    [
      { path: ANNEX, reason: "cloud_only" },
      { path: "Bank statement.pdf", reason: "over_size_cap" },
    ],
  );
  expect(notices()).toEqual([
    { tone: "failed", text: plain(cantUndoWords("en", ANNEX, "cloud_only", "local")) },
    {
      tone: "failed",
      text: plain(cantUndoWords("en", "Bank statement.pdf", "over_size_cap", "local")),
    },
  ]);
  expect(notices().some((n) => n.text.includes("went back"))).toBe(false);
  // the note says nothing of its own: no paper went back
  await vi.waitFor(() => expect(h?.all("cant-undo")).toHaveLength(2));
  expect(noteLines()).not.toContain("You undid");
  expect(noteLines()).toContain("Your cloud service's version history may have the earlier one.");
}, 30_000);

test("a marker without unversioned keeps the lines as they were", async () => {
  h = await undone(["brief.txt"], ONE_DRIVE);
  await hostUndid(h, ["brief.txt"], ["brief.txt"], []);
  expect(notices()).toHaveLength(1);
  expect(notices()[0]?.text).toContain("brief.txt went back");
  expect(h.all("cant-undo")).toHaveLength(0);
  expect(noteLines()).toContain("You undid Clauseo's changes to 1 paper.");
}, 30_000);
