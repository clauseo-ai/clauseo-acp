// @vitest-environment happy-dom
// Restoring a version: the dialog lists exactly the files the host's preview names, the restore goes ahead with
// restore_version, and its Undo restores the version just before it; Word holding a file says which with Try again;
// and comparing with now shows a PDF's sentence on PDFs only.
import { errorWords } from "@clauseo/components/lib/words-errors.ts";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { MATTER, THREAD, TURN, upsert, uuid } from "../../testing/stream-script.ts";
import type { HostQuery, RestorePreview, ViewEntry } from "../../transport/protocol.ts";
import {
  file,
  MATTER_STREAM,
  mountVersions,
  NOON,
  stepEntry,
  version,
  type Script,
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

const lookAt = async (h: VersionsHarness, checkpoint: string) =>
  h.click(h.all("version").find((r) => r.dataset["checkpoint"] === checkpoint));
const dialog = () => document.body.querySelector<HTMLElement>('[role="dialog"]');
const describedBy = () => {
  const id = dialog()?.getAttribute("aria-describedby");
  return id == null ? null : (document.getElementById(id)?.textContent ?? null);
};
const queried = (h: VersionsHarness, kind: keyof Exclude<HostQuery, string>) =>
  h.transport
    .only("query")
    .map((c) => c.query)
    .filter((q) => typeof q === "object" && q[kind] !== undefined);
const timeOfRow = (h: VersionsHarness, checkpoint: string) =>
  h
    .all("version")
    .find((r) => r.dataset["checkpoint"] === checkpoint)
    ?.querySelector(".ver-entry-time")?.textContent ?? "";

const answering =
  (
    answers: Partial<Record<keyof Exclude<HostQuery, string>, unknown>>,
  ): NonNullable<Script["query"]> =>
  (q) => {
    if (typeof q !== "object") return undefined;
    for (const [k, v] of Object.entries(answers))
      if ((q as Record<string, unknown>)[k] !== undefined) return v as never;
    return undefined;
  };

for (const [name, path, pdf] of [
  ["a PDF", "exhibit.pdf", true],
  ["a Word document", "reply.docx", false],
] as const) {
  test(`comparing with now shows ${name} on the same sheet, with the PDF's sentence on PDFs only`, async () => {
    h = await mountVersions(VERSIONS, STEPS, {
      query: answering({ compare_versions: { file_changes: [file(path)] } }),
    });
    await h.openVersions();
    await lookAt(h, "cp2");
    const compare = h.button("Compare with now");
    await h.click(compare);
    expect(compare?.getAttribute("aria-pressed")).toBe("true");
    expect(queried(h, "compare_versions")).toEqual([
      { compare_versions: { matter: MATTER, from: "cp2" } },
    ]);
    const sheet = document.body.querySelector(".ver-pdf");
    expect(sheet).not.toBeNull();
    expect(sheet?.textContent).toContain(path);
    const sentence = "A PDF shows only whether it changed. Open each to read it.";
    if (pdf) expect(document.body.textContent).toContain(sentence);
    else expect(document.body.textContent).not.toContain(sentence);
  });
}

const PREVIEW: RestorePreview = {
  changes: [file("brief.txt"), file("notes.docx")],
  added_since: ["new.txt"],
  kept: [],
  locked: [],
};

test("the restore dialog lists exactly the preview's files, restores, and its Undo asks to restore the version before", async () => {
  h = await mountVersions(VERSIONS, STEPS, {
    query: answering({ preview_restore: { restore_preview: PREVIEW } }),
    dispatch: (c) =>
      c.restore_version?.checkpoint === "cp2"
        ? { version_restored: { checkpoint: "cp4" } }
        : { version_restored: { checkpoint: "cp5" } },
  });
  await h.openVersions();
  const at2 = timeOfRow(h, "cp2");
  await lookAt(h, "cp2");
  await h.click(h.one("version-restore"));
  expect(queried(h, "preview_restore")).toEqual([
    { preview_restore: { matter: MATTER, checkpoint: "cp2", scope: "matter" } },
  ]);
  expect(dialog()?.textContent).toContain(`Restore the matter to how it was at ${at2} today?`);
  expect(describedBy()).toBe(
    "2 documents will change: \u2068brief.txt\u2069 and \u2068notes.docx\u2069. You can undo this.",
  );
  expect(dialog()?.querySelector(".ver-dialog-note")?.textContent).toBe(
    "'\u2068new.txt\u2069', added since, stays.",
  );
  // the step's own file list is not the dialog's: only the preview's two files are named
  expect(dialog()?.textContent).not.toContain("other.txt");

  await h.click(h.one("dialog-confirm"));
  expect(h.sent("restore_version")).toEqual([
    { matter: MATTER, checkpoint: "cp2", scope: "matter" },
  ]);
  // the host's restore version, and its marker, on the matter's stream
  await h.deliver(MATTER_STREAM, [
    upsert(version(4, "restore")),
    upsert({
      marker: {
        order: 40,
        event: uuid(40, 9),
        ts: NOON + 4 * 60_000,
        kind: { version_restored: { from: "cp2", checkpoint: "cp4", scope: "matter" } },
      },
    }),
  ]);
  expect(dialog()).toBeNull();
  // cp3 was now before the restore; its time shows on its row once the restore is newer
  const at3 = timeOfRow(h, "cp3");
  expect(at3).toMatch(/\d/);
  const done = [...document.body.querySelectorAll(".ver-notice")].find((n) =>
    n.textContent?.includes("Restored to"),
  );
  expect(done?.querySelector(".ver-notice-text")?.textContent).toBe(
    `Restored to ${at2} today. 2 documents changed.`,
  );
  // the restore's own row names what it restored to
  expect(
    h
      .all("version")
      .find((r) => r.dataset["checkpoint"] === "cp4")
      ?.querySelector(".ver-entry-who")?.textContent,
  ).toBe(`Restore to ${at2} today`);

  await h.click(done?.querySelector(".ver-restored-undo"));
  // the Undo asks first, with what restoring the version before writes now
  expect(queried(h, "preview_restore").at(-1)).toEqual({
    preview_restore: { matter: MATTER, checkpoint: "cp3", scope: "matter" },
  });
  expect(describedBy()).toBe(
    "2 documents will change: \u2068brief.txt\u2069 and \u2068notes.docx\u2069. You can undo this.",
  );
  expect(h.sent("restore_version")).toHaveLength(1);
  await h.click(h.one("dialog-confirm"));
  expect(h.sent("restore_version")).toEqual([
    { matter: MATTER, checkpoint: "cp2", scope: "matter" },
    { matter: MATTER, checkpoint: "cp3", scope: "matter" },
  ]);
  expect(document.body.textContent).toContain(
    `Undid the restore. The matter stands as it did at ${at3}.`,
  );
});

test("Word holding a file: the dialog names it with Try again, the page says why, and Try again previews and restores", async () => {
  let held = true;
  h = await mountVersions(VERSIONS, STEPS, {
    query: answering({
      preview_restore: {
        restore_preview: {
          changes: [file("brief.txt"), file("notes.docx")],
          added_since: [],
          kept: [],
          locked: ["brief.txt"],
        },
      },
    }),
    dispatch: () => {
      if (held) throw { code: "matter.file_locked", message: "held", retryable: true };
      return { version_restored: { checkpoint: "cp4" } };
    },
  });
  await h.openVersions();
  await lookAt(h, "cp2");
  await h.click(h.one("version-restore"));
  await h.click(h.one("dialog-confirm"));
  expect(h.sent("restore_version")).toHaveLength(1);
  expect(dialog()?.textContent).toContain("Close '\u2068brief.txt\u2069' in Word to restore it.");
  expect(h.one("dialog-try-again")).not.toBeNull();
  expect(h.one("dialog-confirm")).toBeNull();
  // under the dialog, the page says why by the refusal's code
  const page = [...document.body.querySelectorAll(".ver-room-band")].map((n) => n.textContent);
  expect(page.join(" ")).toContain(errorWords("en", "matter.file_locked"));

  held = false;
  await h.click(h.one("dialog-try-again"));
  expect(queried(h, "preview_restore")).toHaveLength(2);
  expect(h.sent("restore_version")).toEqual([
    { matter: MATTER, checkpoint: "cp2", scope: "matter" },
    { matter: MATTER, checkpoint: "cp2", scope: "matter" },
  ]);
  expect(dialog()).toBeNull();
  expect(document.body.textContent).not.toContain(errorWords("en", "matter.file_locked"));
  expect(document.body.textContent).toMatch(/Restored to .+ today\. 2 documents changed\./);
});
