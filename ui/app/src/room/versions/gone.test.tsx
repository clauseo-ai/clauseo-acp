// @vitest-environment happy-dom
// A turn during which files Clauseo kept no copy of were deleted (the step's gone, from FilesChanged): the timeline's
// short note on that change, and, picked, the band's line for each file, the same words the thread's notice says.
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { THREAD, TURN } from "../../testing/stream-script.ts";
import type { FolderPlace, SkippedFile, ViewEntry } from "../../transport/protocol.ts";
import { detailed, fakeNext } from "../fake-next.ts";
import {
  file,
  mountVersions,
  NOON,
  stepEntry,
  THE_MATTER,
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

/** The words without the isolates the slots are wrapped in. */
const plain = (s: string | null | undefined) => (s ?? "").replace(/[\u2068\u2069]/g, "");

const VERSIONS: ViewEntry[] = [
  version(1, "import"),
  version(2, "after_turn", { thread: THREAD, turn: TURN }),
];

async function mounted(gone: SkippedFile[], place: FolderPlace): Promise<VersionsHarness> {
  const harness = await mountVersions(
    VERSIONS,
    [stepEntry(5, { files: [file("brief.txt")], gone, checkpoint_after: "cp2" })],
    {
      next: fakeNext({
        matters: () => [detailed(THE_MATTER)],
        parts: ["papers"],
        listing: { place },
      }),
    },
  );
  await harness.openVersions();
  return harness;
}

const band = () => plain(document.body.querySelector(".ver-note")?.textContent);

test("a deleted file only in OneDrive: the timeline notes it, and the band says OneDrive's recycle bin may have it", async () => {
  h = await mounted([{ path: "Drafts/Brief.docx", reason: "cloud_only" }], {
    synced: { service: "one_drive" },
  });
  await vi.waitFor(() => expect(h?.all("ver-gone")).toHaveLength(1));
  expect(plain(h.one("ver-gone")?.textContent)).toBe(
    "Brief.docx was deleted, with no copy to bring back",
  );
  await h.lookAt("cp2");
  await vi.waitFor(() =>
    expect(band()).toBe(
      "Brief.docx was deleted during this step. Clauseo had no copy of it to bring back. It was only in OneDrive, so OneDrive's recycle bin may still have it.",
    ),
  );
}, 30_000);

test("a deleted file too large to keep: no recycle bin; a version with none gone has no note", async () => {
  h = await mounted([{ path: "Scans/Photos.zip", reason: "over_size_cap" }], "local");
  await h.lookAt("cp2");
  await vi.waitFor(() =>
    expect(band()).toBe(
      "Photos.zip was deleted during this step. Clauseo had no copy of it to bring back, because it was too large to keep versions of.",
    ),
  );
  expect(band()).not.toContain("recycle bin");
  await h.unmount();
  h = await mounted([], "local");
  expect(h.all("ver-gone")).toHaveLength(0);
  await h.lookAt("cp2");
  expect(document.body.querySelector(".ver-note")).toBeNull();
}, 30_000);
