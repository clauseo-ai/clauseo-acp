// @vitest-environment happy-dom
// Every file name Versions shows is isolated from the words around it (FSI…PDI), so a right-to-left override in a
// name can't reorder the sentence it stands in: on the timeline, in the band, in the dialog and in the lines after.
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { THREAD, TURN } from "../../testing/stream-script.ts";
import {
  answering,
  describedBy,
  dialog,
  file,
  mountVersions,
  NOON,
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

const SPOOF = "notes/a\u202Efdp.exe";
const NAME = "a\u202Efdp.exe";
const ISOLATED = `\u2068${NAME}\u2069`;

/** Each place the name stands in the text, with whether FSI is just before it and PDI just after. */
const isolatedEach = (text: string | null | undefined): boolean[] =>
  [...(text ?? "").matchAll(new RegExp(NAME, "g"))].map(
    (m) => text?.[m.index - 1] === "\u2068" && text[m.index + NAME.length] === "\u2069",
  );

test("a file name with a right-to-left override is isolated on the timeline, in the band and in the dialogs", async () => {
  h = await mountVersions(
    [
      version(1, "import"),
      version(2, "after_turn", { thread: THREAD, turn: TURN }),
      version(3, "outside_edits", { files: [file(SPOOF)] }),
      version(4, "import"),
    ],
    [stepEntry(5, { files: [file(SPOOF), file("brief.txt")], checkpoint_after: "cp2" })],
    {
      query: answering({
        preview_restore: {
          restore_preview: { changes: [file(SPOOF)], added_since: [SPOOF], kept: [], locked: [] },
        },
      }),
    },
  );
  await h.openVersions();
  const rows = h.one("versions")?.textContent;
  expect(isolatedEach(rows).length).toBeGreaterThan(0);
  expect(isolatedEach(rows).every(Boolean)).toBe(true);
  // the band, looking back at the lawyer's edit
  await h.lookAt("cp3");
  const band = document.body.querySelector(".ver-band")?.textContent;
  expect(band).toContain(ISOLATED);
  expect(isolatedEach(band).every(Boolean)).toBe(true);
  // the restore dialog
  await h.click(h.one("version-restore"));
  expect(describedBy()).toContain(ISOLATED);
  expect(isolatedEach(dialog()?.textContent).every(Boolean)).toBe(true);
  await h.click(h.button("Cancel", dialog()));
  // the undo dialog of the turn that changed it
  await h.lookAt("cp2");
  await h.click(h.one("version-undo"));
  expect(isolatedEach(dialog()?.textContent).length).toBeGreaterThan(0);
  expect(isolatedEach(dialog()?.textContent).every(Boolean)).toBe(true);
});
