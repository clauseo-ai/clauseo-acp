// @vitest-environment happy-dom
// The restore dialog keeps to what restoring writes now: the matter moving while it asks re-reads the preview and
// replaces its list; Try again goes back to the dialog when the preview changed; only the newest ask opens it. A
// program holding a file nobody named: a restore asks the preview which, an undo keeps its choices with Try again.
// And two files of one name show their paths.
import { errorWords } from "@clauseo/components/lib/words-errors.ts";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { THREAD, TURN, upsert } from "../../testing/stream-script.ts";
import type { RestorePreview, ViewEntry } from "../../transport/protocol.ts";
import {
  answering,
  describedBy,
  dialog,
  file,
  MATTER_STREAM,
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

const VERSIONS: ViewEntry[] = [
  version(1, "import"),
  version(2, "after_turn", { thread: THREAD, turn: TURN }),
  version(3, "outside_edits", { files: [file("other.txt")] }),
];
const STEPS = [stepEntry(5, { files: [file("brief.txt")], checkpoint_after: "cp2" })];
const previewOf = (paths: string[], locked: string[] = []): RestorePreview => ({
  changes: paths.map((p) => file(p)),
  added_since: [],
  kept: [],
  locked,
});
const locked = { code: "matter.file_locked", message: "held", retryable: true };

test("the matter moving while the restore dialog asks re-reads the preview, and the dialog lists the new files", async () => {
  let preview = previewOf(["brief.txt"]);
  h = await mountVersions(VERSIONS, STEPS, {
    query: answering({ preview_restore: () => ({ restore_preview: preview }) }),
    dispatch: () => ({ version_restored: { checkpoint: "cp5" } }),
  });
  await h.openVersions();
  await h.lookAt("cp2");
  await h.click(h.one("version-restore"));
  expect(describedBy()).toBe("1 document will change: \u2068brief.txt\u2069. You can undo this.");
  // a turn ends elsewhere and changes memo.docx: restoring cp2 now writes it back too
  preview = previewOf(["brief.txt", "memo.docx"]);
  await h.deliver(MATTER_STREAM, [
    upsert(version(4, "outside_edits", { files: [file("memo.docx")] })),
  ]);
  expect(h.queried("preview_restore")).toHaveLength(2);
  expect(describedBy()).toBe(
    "2 documents will change: \u2068brief.txt\u2069 and \u2068memo.docx\u2069. You can undo this.",
  );
  await h.click(h.one("dialog-confirm"));
  expect(document.body.textContent).toMatch(/Restored to .+ today\. 2 documents changed\./);
});

test("Try again after Word let go goes back to the dialog when what the restore writes has changed", async () => {
  let preview = previewOf(["brief.txt", "notes.docx"], ["brief.txt"]);
  let held = true;
  h = await mountVersions(VERSIONS, STEPS, {
    query: answering({ preview_restore: () => ({ restore_preview: preview }) }),
    dispatch: () => {
      if (held) throw locked;
      return { version_restored: { checkpoint: "cp5" } };
    },
  });
  await h.openVersions();
  await h.lookAt("cp2");
  await h.click(h.one("version-restore"));
  await h.click(h.one("dialog-confirm"));
  expect(dialog()?.textContent).toContain("Close '\u2068brief.txt\u2069' in Word to restore it.");
  held = false;
  preview = previewOf(["brief.txt", "notes.docx", "memo.docx"]);
  await h.click(h.one("dialog-try-again"));
  // the restore waits: the dialog lists the three files for the lawyer to read again
  expect(h.sent("restore_version")).toHaveLength(1);
  expect(describedBy()).toBe(
    "3 documents will change: \u2068brief.txt\u2069, \u2068notes.docx\u2069 and \u2068memo.docx\u2069. You can undo this.",
  );
  await h.click(h.one("dialog-confirm"));
  expect(h.sent("restore_version")).toHaveLength(2);
  expect(dialog()).toBeNull();
});

test("only the newest restore asked for opens the dialog, whichever preview comes back last", async () => {
  const waiting = new Map<string, (p: RestorePreview) => void>();
  h = await mountVersions(VERSIONS, STEPS, {
    query: answering({
      preview_restore: (q: { preview_restore: { checkpoint: string } }) =>
        new Promise((resolve) =>
          waiting.set(q.preview_restore.checkpoint, (p) => resolve({ restore_preview: p })),
        ),
    }),
  });
  await h.openVersions();
  await h.lookAt("cp2");
  await h.click(h.one("version-restore"));
  await h.lookAt("cp1");
  await h.click(h.one("version-restore"));
  waiting.get("cp1")?.(previewOf(["brief.txt", "other.txt"]));
  await h.settle();
  waiting.get("cp2")?.(previewOf(["brief.txt"]));
  await h.settle();
  expect(describedBy()).toBe(
    "2 documents will change: \u2068brief.txt\u2069 and \u2068other.txt\u2069. You can undo this.",
  );
});

test("Word holding a file no one named: the restore asks the preview again, and its dialog names the file", async () => {
  let preview = previewOf(["brief.txt", "notes.docx"]);
  h = await mountVersions(VERSIONS, STEPS, {
    query: answering({ preview_restore: () => ({ restore_preview: preview }) }),
    dispatch: () => {
      preview = previewOf(["brief.txt", "notes.docx"], ["notes.docx"]);
      throw locked;
    },
  });
  await h.openVersions();
  await h.lookAt("cp2");
  await h.click(h.one("version-restore"));
  await h.click(h.one("dialog-confirm"));
  expect(h.queried("preview_restore")).toHaveLength(2);
  expect(dialog()?.textContent).toContain("Close '\u2068notes.docx\u2069' in Word to restore it.");
  expect(h.one("dialog-try-again")).not.toBeNull();
});

test("Word holding a file of an undo of two: the dialog keeps the choices, says why, and Try again sends the same undo", async () => {
  let held = true;
  h = await mountVersions(
    VERSIONS,
    [
      stepEntry(5, {
        files: [file("brief.txt"), file("notes.docx", { may_include_lawyer_edits: true })],
        checkpoint_after: "cp2",
      }),
    ],
    {
      dispatch: () => {
        if (held) throw locked;
        return { step_undone: { checkpoint: "cp5" } };
      },
    },
  );
  await h.openVersions();
  await h.lookAt("cp2");
  await h.click(h.one("version-undo"));
  await h.click(h.button("Undo anyway", dialog()));
  await h.click(h.one("dialog-confirm"));
  const files = [
    { path: "brief.txt", choice: "undo" },
    { path: "notes.docx", choice: "undo" },
  ];
  expect(h.sent("undo_step")).toEqual([{ thread: THREAD, turn: TURN, files }]);
  // the dialog stays, with the lawyer's choice and the reason, and Try again in place of Undo
  expect(dialog()).not.toBeNull();
  expect(dialog()?.querySelector('[role="alert"]')?.textContent).toBe(
    errorWords("en", "matter.file_locked"),
  );
  expect(h.button("Undo anyway", dialog())?.getAttribute("aria-pressed")).toBe("true");
  expect(h.one("dialog-confirm")).toBeNull();
  held = false;
  await h.click(h.one("dialog-try-again"));
  expect(h.sent("undo_step")).toEqual([
    { thread: THREAD, turn: TURN, files },
    { thread: THREAD, turn: TURN, files },
  ]);
  expect(dialog()).toBeNull();
});

test("two files of one name in the restore dialog show their paths, isolated from the words around them", async () => {
  h = await mountVersions(VERSIONS, STEPS, {
    query: answering({
      preview_restore: { restore_preview: previewOf(["a/brief.txt", "b/brief.txt", "memo.docx"]) },
    }),
  });
  await h.openVersions();
  await h.lookAt("cp2");
  await h.click(h.one("version-restore"));
  expect(describedBy()).toBe(
    "3 documents will change: \u2068a/brief.txt\u2069, \u2068b/brief.txt\u2069 and \u2068memo.docx\u2069. You can undo this.",
  );
});

test("the restore dialog says what stays in the plural for two documents, in the singular for one", async () => {
  h = await mountVersions(VERSIONS, STEPS, {
    query: answering({
      preview_restore: {
        restore_preview: {
          changes: [file("brief.txt")],
          added_since: ["r.docx", "s.docx"],
          kept: ["a.pdf", "b.pdf"],
          locked: [],
        },
      },
    }),
  });
  await h.openVersions();
  await h.lookAt("cp2");
  await h.click(h.one("version-restore"));
  const note = dialog()?.querySelector(".ver-dialog-note")?.textContent ?? "";
  // each name in quotes of its own, "and" outside them
  expect(note).toContain("'\u2068r.docx\u2069' and '\u2068s.docx\u2069', added since, stay.");
  expect(note).toContain(
    "'\u2068a.pdf\u2069' and '\u2068b.pdf\u2069' can't be restored and stay as they are.",
  );
  expect(note).not.toContain("stays");
});

test("in Hindi too, each name of what stays is quoted, and और joins them outside the quotes", async () => {
  h = await mountVersions(VERSIONS, STEPS, {
    query: answering({
      preview_restore: {
        restore_preview: {
          changes: [file("brief.txt")],
          added_since: ["r.docx", "s.docx"],
          kept: ["a.pdf", "b.pdf"],
          locked: [],
        },
      },
    }),
    display: { language: "hi" },
  });
  await h.openVersions();
  await h.lookAt("cp2");
  await h.click(h.one("version-restore"));
  const note = dialog()?.querySelector(".ver-dialog-note")?.textContent ?? "";
  expect(note).toContain("बाद में जोड़े गए '\u2068r.docx\u2069' और '\u2068s.docx\u2069' बने रहेंगे।");
  expect(note).toContain(
    "'\u2068a.pdf\u2069' और '\u2068b.pdf\u2069' पुनर्स्थापित नहीं हो सकते और जैसे हैं वैसे ही रहेंगे।",
  );
});
