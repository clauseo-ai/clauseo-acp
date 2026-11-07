// @vitest-environment happy-dom Versions' own acts through the host, refused: Open now and Show
// (where versions are kept) say why on the band, by the code's words, as a refused restore does; a
// declined confirm (transport.unauthorized) says nothing; a folder gone or not the matter's offers
// Locate folder beside the words, a restore's refusal too; trying again clears the line.
import { errorWords } from "@clauseo/components/lib/words-errors.ts";
import { afterEach, expect, test, vi } from "vitest";
import { THREAD, TURN, uuid } from "../../testing/stream-script.ts";
import type { HostQuery, ViewEntry } from "../../transport/protocol.ts";
import { locatable } from "../act-refusals.ts";
import { fakeNext } from "../fake-next.ts";
import { file, mountVersions, stepEntry, version, type VersionsHarness } from "./testing.ts";

let h: VersionsHarness | null = null;
afterEach(async () => {
  await h?.unmount();
  h = null;
  vi.restoreAllMocks();
});

const refusal = (code: string) => ({ code, message: "C:\\Matters\\for logs", retryable: false });
const CODES = [
  "internal",
  "some.newer_code",
  "matter.folder_missing",
  "matter.folder_not_a_matter",
] as const;
const REF = uuid(1, 8);
const VERSIONS: ViewEntry[] = [
  version(1, "import"),
  version(2, "after_turn", { thread: THREAD, turn: TURN }),
  version(3, "outside_edits", { files: [file("exhibit.pdf")] }),
];
const STEPS = [stepEntry(5, { files: [file("exhibit.pdf")], checkpoint_after: "cp2" })];

/** Versions looking back at cp2, compared with now, with Open now through the stand-in
 * `openFile`. */
async function comparing(openFile: () => Promise<{ protected_view: boolean }>) {
  const room = await mountVersions(VERSIONS, STEPS, {
    query: (q) =>
      typeof q === "object" && q.compare_versions !== undefined
        ? { file_changes: [file("exhibit.pdf")] }
        : undefined,
    next: fakeNext({
      matters: () => [],
      papers: [{ file: REF, path: "exhibit.pdf", size: 10, modified: 0 }],
      parts: ["papers"],
    }),
    standIns: { openFile },
  });
  await room.openVersions();
  await room.lookAt("cp2");
  await room.click(room.button("Compare with now"));
  return room;
}

const note = () => document.body.querySelector<HTMLElement>(".ver-note");
const locate = () => document.body.querySelector<HTMLElement>('[data-testid="locate-folder"]');
const failedNotice = () =>
  document.body.querySelector<HTMLElement>('.ver-notice[data-tone="failed"]');

for (const code of CODES)
  test(`a refused Open now says why on the band, by its code, until it's tried again: ${code}`, async () => {
    vi.spyOn(console, "warn").mockImplementation(() => {});
    let fail: string | null = code;
    h = await comparing(() =>
      fail === null ? Promise.resolve({ protected_view: false }) : Promise.reject(refusal(fail)),
    );
    await h.click(h.button("Open now"));
    expect(note()?.querySelector("span")?.textContent).toBe(errorWords("en", code));
    expect(document.body.textContent).not.toContain("for logs");
    expect(locate() !== null).toBe(locatable(code));
    if (locatable(code)) {
      await h.click(locate());
      expect(h.transport.only("pick_folder")).toHaveLength(1);
    }
    fail = null;
    await h.click(h.button("Open now"));
    expect(note()).toBeNull();
    expect(locate()).toBeNull();
  });

test("a declined Open now says nothing", async () => {
  h = await comparing(() => Promise.reject(refusal("transport.unauthorized")));
  await h.click(h.button("Open now"));
  expect(note()).toBeNull();
});

const showButton = () =>
  [...document.body.querySelectorAll(".ver-where button")].find((b) =>
    (b.textContent ?? "").startsWith("Show in"),
  );

test("a refused Show says why on the band at now; declined it says nothing; a show that goes through clears it", async () => {
  vi.spyOn(console, "warn").mockImplementation(() => {});
  let fail: string | null = "internal";
  h = await mountVersions([version(1, "import")], [], {
    standIns: {
      reveal: () => (fail === null ? Promise.resolve() : Promise.reject(refusal(fail))),
    },
  });
  await h.openVersions();
  await h.click(document.body.querySelector(".ver-foot"));
  await h.click(showButton());
  expect(failedNotice()?.textContent).toBe(errorWords("en", "internal"));
  fail = "transport.unauthorized";
  await h.click(showButton());
  expect(failedNotice()).toBeNull();
  fail = "matter.folder_not_a_matter";
  await h.click(showButton());
  expect(failedNotice()?.textContent).toContain(errorWords("en", fail));
  expect(failedNotice()?.querySelector('[data-testid="locate-folder"]')).not.toBeNull();
  fail = null;
  await h.click(showButton());
  expect(failedNotice()).toBeNull();
});

for (const code of ["matter.folder_missing", "matter.folder_not_a_matter"])
  test(`a restore refused with ${code} offers Locate folder beside its words`, async () => {
    vi.spyOn(console, "warn").mockImplementation(() => {});
    h = await mountVersions(VERSIONS, STEPS, {
      query: (q) =>
        typeof q === "object" && (q as HostQuery & object).preview_restore !== undefined
          ? Promise.reject(refusal(code))
          : undefined,
    });
    await h.openVersions();
    await h.lookAt("cp2");
    await h.click(h.one("version-restore"));
    expect(document.body.textContent).toContain(errorWords("en", code));
    expect(locate()?.textContent).toBe("Locate folder");
  });
