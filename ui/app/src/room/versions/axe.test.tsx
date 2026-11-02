// @vitest-environment happy-dom
// axe-core over Versions in the room, in both languages: the timeline with a not-saved row, the band at now with its
// notices, the band looking back, the undo dialog with a row to choose for, the restore dialog, and the dialog that
// says Word holds a file. happy-dom computes no styles, so colour contrast is left off, as in the room's axe test
// (axe.test.tsx); it is checked by hand in the app.
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import axe from "axe-core";
import { afterEach, beforeAll, beforeEach, expect, test, vi } from "vitest";
import { THREAD, TURN, uuid } from "../../testing/stream-script.ts";
import type { ViewEntry } from "../../transport/protocol.ts";
import { file, mountVersions, NOON, stepEntry, version, type VersionsHarness } from "./testing.ts";

let h: VersionsHarness | null = null;
beforeAll(() => {
  // the page's own title, as index.html gives it
  const html = readFileSync(
    join(dirname(fileURLToPath(import.meta.url)), "../../../index.html"),
    "utf8",
  );
  document.title = /<title>([^<]*)<\/title>/.exec(html)?.[1] ?? "";
});
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
const STEPS: ViewEntry[] = [
  stepEntry(5, {
    files: [file("a.docx", { may_include_lawyer_edits: true }), file("b.docx")],
    skipped: [{ path: "exhibits.pdf", reason: "over_size_cap" }],
    checkpoint_after: "cp2",
  }),
  // a later turn whose change couldn't be kept: the not-saved row and notice
  stepEntry(9, { turn: uuid(9, 4), files: [file("late.txt")] }),
];

async function violations(scene: string): Promise<string[]> {
  const results = await axe.run(document, {
    resultTypes: ["violations"],
    rules: { "color-contrast": { enabled: false } },
  });
  return results.violations.map(
    (v) => `${scene}: ${v.id} at ${v.nodes.map((n) => n.target.join(" ")).join(", ")}`,
  );
}

for (const language of ["en", "hi"] as const) {
  test(`axe finds no violation in Versions (${language})`, async () => {
    h = await mountVersions(VERSIONS, STEPS, {
      display: { language },
      query: (q) =>
        typeof q === "object" && q.preview_restore !== undefined
          ? {
              restore_preview: {
                changes: [file("brief.txt"), file("notes.docx")],
                added_since: ["new.txt"],
                kept: [],
                locked: ["notes.docx"],
              },
            }
          : undefined,
      dispatch: () => {
        throw { code: "matter.file_locked", message: "held", retryable: true };
      },
    });
    await h.openVersions();
    const found: string[] = [];
    // the timeline, and the band at now with the not-saved notice
    expect(h.one("versions")?.querySelector(".ver-unsaved")).not.toBeNull();
    expect(document.body.querySelector(".ver-notice")).not.toBeNull();
    found.push(...(await violations("at now")));

    await h.click(h.all("version").find((r) => r.dataset["checkpoint"] === "cp2"));
    expect(h.one("version-undo")).not.toBeNull();
    found.push(...(await violations("looking back")));

    await h.click(h.one("version-undo"));
    expect(document.body.querySelector(".ver-edited-row")).not.toBeNull();
    found.push(...(await violations("the undo dialog")));
    // Cancel, the dialog's first act
    await h.click(document.body.querySelector('[role="dialog"] [data-dialog-acts] button'));
    expect(document.body.querySelector('[role="dialog"]')).toBeNull();

    await h.click(h.one("version-restore"));
    expect(document.body.querySelector(".ver-dialog-note")).not.toBeNull();
    found.push(...(await violations("the restore dialog")));

    await h.click(h.one("dialog-confirm"));
    expect(h.one("dialog-try-again")).not.toBeNull();
    found.push(...(await violations("Word holds a file")));

    expect(found).toEqual([]);
  });
}
