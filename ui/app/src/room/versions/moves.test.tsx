// @vitest-environment happy-dom
// What a move of the matter does to what Versions shows: a newest version picked at now is looked back at once a newer
// one lands; the undo dialog keeps each file changed since, unless the lawyer chose for it; the comparison is asked
// for again; a newest turn's step that came a flush late takes the toolbar's dialog; a step undone in a conversation
// the room holds counts; and the answers that can't be read, or that come after Cancel, say so or stay quiet.
import { errorWords } from "@clauseo/components/lib/words-errors.ts";
import { act } from "react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { STREAM, THREAD, TURN, upsert, uuid } from "../../testing/stream-script.ts";
import type { FileChange, RestorePreview, ViewEntry } from "../../transport/protocol.ts";
import {
  answering,
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

const LATER = uuid(2, 4);
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

const rowOf = (h: VersionsHarness, checkpoint: string) =>
  h.all("version").find((r) => r.dataset["checkpoint"] === checkpoint);
const undoButton = (h: VersionsHarness) => h.host.querySelector(".room-pair .room-undo");
const outside = (n: number, ...paths: string[]) =>
  upsert(version(n, "outside_edits", { files: paths.map((p) => file(p)) }));

test("the newest turn's version, picked at now, is looked back at once a newer version lands", async () => {
  h = await mountVersions(
    [...VERSIONS.slice(0, 2), version(3, "after_turn", { thread: THREAD, turn: LATER })],
    [...STEPS, stepEntry(9, { turn: LATER, files: [file("memo.docx")], checkpoint_after: "cp3" })],
  );
  await h.openVersions();
  await h.lookAt("cp3");
  expect(document.body.querySelector(".ver-line")?.textContent).toBe("Now");
  await h.deliver(MATTER_STREAM, [outside(4, "x.txt")]);
  // the band and the panel agree: cp3, an older version now, with its Restore
  expect(document.body.querySelector(".ver-line")?.textContent).not.toBe("Now");
  expect(h.one("version-restore")).not.toBeNull();
  expect(rowOf(h, "cp3")?.getAttribute("aria-current")).toBe("true");
});

test("a move of the matter while the undo dialog asks keeps each file changed since, unless the lawyer chose", async () => {
  h = await mountVersions(VERSIONS, [
    stepEntry(5, {
      files: [file("a.docx", { may_include_lawyer_edits: true }), file("b.docx")],
      checkpoint_after: "cp2",
    }),
  ]);
  await h.openVersions();
  await h.lookAt("cp2");
  await h.click(h.one("version-undo"));
  await h.click(h.button("Undo anyway", dialog()));
  // the lawyer edits both outside Clauseo while the dialog asks
  await h.deliver(MATTER_STREAM, [outside(4, "a.docx", "b.docx")]);
  await h.click(h.one("dialog-confirm"));
  expect(h.sent("undo_step")).toEqual([
    {
      thread: THREAD,
      turn: TURN,
      files: [
        { path: "a.docx", choice: "undo" },
        { path: "b.docx", choice: "keep" },
      ],
    },
  ]);
});

test("comparing with now asks again when the matter moves, and lists the new answer", async () => {
  let changes: FileChange[] = [file("brief.txt")];
  h = await mountVersions(VERSIONS, STEPS, {
    query: answering({ compare_versions: () => ({ file_changes: changes }) }),
  });
  await h.openVersions();
  await h.lookAt("cp2");
  await h.click(h.button("Compare with now"));
  expect(h.queried("compare_versions")).toHaveLength(1);
  expect(document.body.textContent).not.toContain("memo.docx");
  changes = [file("brief.txt"), file("memo.docx")];
  await h.deliver(MATTER_STREAM, [outside(4, "memo.docx")]);
  expect(h.queried("compare_versions")).toHaveLength(2);
  expect(document.body.textContent).toContain("memo.docx");
});

test("a comparison answered in the wrong shape says it failed, never that nothing changed", async () => {
  h = await mountVersions(VERSIONS, STEPS, {
    query: answering({ compare_versions: { restore_preview: PREVIEW } }),
  });
  await h.openVersions();
  await h.lookAt("cp2");
  await h.click(h.button("Compare with now"));
  expect(document.body.textContent).not.toContain("No changes since then.");
  expect(document.body.textContent).toContain(errorWords("en", "internal"));
});

test("the newest turn's step coming a flush after its version takes the dialog the toolbar's Undo opened", async () => {
  const versions = [
    ...VERSIONS.slice(0, 2),
    version(3, "after_turn", { thread: THREAD, turn: LATER }),
  ];
  h = await mountVersions(versions, STEPS);
  await h.openVersions();
  await h.click(undoButton(h));
  // cp3's step isn't in: the walk went on to cp2
  expect(rowOf(h, "cp2")?.getAttribute("aria-current")).toBe("true");
  await h.deliver(STREAM, [
    upsert(stepEntry(9, { turn: LATER, files: [file("memo.docx")], checkpoint_after: "cp3" })),
  ]);
  expect(rowOf(h, "cp3")?.getAttribute("aria-current")).toBe("true");
  await h.click(h.one("dialog-confirm"));
  expect(h.sent("undo_step")).toEqual([
    { thread: THREAD, turn: LATER, files: [{ path: "memo.docx", choice: "undo" }] },
  ]);
});

test("a step undone in a later flush of a conversation the room holds leaves the toolbar's Undo nothing", async () => {
  h = await mountVersions(VERSIONS, STEPS);
  await h.openVersions();
  expect(undoButton(h)?.getAttribute("aria-disabled")).toBeNull();
  await h.deliver(STREAM, [
    upsert(stepEntry(5, { files: [file("brief.txt")], checkpoint_after: "cp2", undone: true })),
  ]);
  expect(undoButton(h)?.getAttribute("aria-disabled")).toBe("true");
});

test("a preview that fails to come back after the matter moved says so in the dialog, with Try again", async () => {
  let fails = false;
  h = await mountVersions(VERSIONS, STEPS, {
    query: answering({
      preview_restore: () => {
        if (fails) throw { code: "internal", message: "gone", retryable: true };
        return { restore_preview: PREVIEW };
      },
    }),
  });
  await h.openVersions();
  await h.lookAt("cp2");
  await h.click(h.one("version-restore"));
  fails = true;
  await h.deliver(MATTER_STREAM, [outside(4, "x.txt")]);
  expect(dialog()?.querySelector('[role="alert"]')?.textContent).toBe(errorWords("en", "internal"));
  expect(h.one("dialog-try-again")).not.toBeNull();
});

test("Cancel while a restore is on its way: Word refusing it afterwards opens no dialog again", async () => {
  let refuse: (error: unknown) => void = () => undefined;
  h = await mountVersions(VERSIONS, STEPS, {
    query: answering({ preview_restore: { restore_preview: PREVIEW } }),
    dispatch: () =>
      new Promise((_, reject) => {
        refuse = reject;
      }),
  });
  await h.openVersions();
  await h.lookAt("cp2");
  await h.click(h.one("version-restore"));
  await h.click(h.one("dialog-confirm"));
  await h.click(h.button("Cancel", dialog()));
  expect(dialog()).toBeNull();
  await act(async () => refuse({ code: "matter.file_locked", message: "held", retryable: true }));
  await h.settle();
  expect(dialog()).toBeNull();
});
