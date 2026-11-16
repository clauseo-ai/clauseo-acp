// @vitest-environment happy-dom
// Restoring one document: comparing a version with now offers Restore this document for the document in view, asks
// preview_restore and restore_version for that file alone, and the dialog says a copy there now is replaced (kept in
// Versions) or a removed one comes back (beside its renamed copy); one added since has nothing to bring back; one that
// would write nothing holds Restore; a file whose name is now a folder is refused in plain words, naming what's in the
// way; the done line says what happened, and an Undo what it did to the document.
import { errorWords } from "@clauseo/components/lib/words-errors.ts";
import { act } from "react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { MATTER, THREAD, TURN, upsert, uuid } from "../../testing/stream-script.ts";
import type { FileChange, RestorePreview, ViewEntry } from "../../transport/protocol.ts";
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
  type Script,
  type VersionsHarness,
} from "./testing.ts";
import { restorable, restoreDone } from "./state.ts";

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
const one = (path: string): RestorePreview => ({
  changes: [file(path)],
  added_since: [],
  kept: [],
  locked: [],
});
const ONE_FILE = { files: { paths: ["brief.txt"] } };

/** Versions open on cp2, compared with now, the folder differing in `changes`; preview: preview_restore's answer (a
 * function may throw a refusal), by default one that writes the first file. */
async function comparing(
  changes: FileChange[],
  { dispatch, preview }: { dispatch?: Script["dispatch"]; preview?: unknown } = {},
): Promise<VersionsHarness> {
  const harness = await mountVersions(VERSIONS, STEPS, {
    ...(dispatch === undefined ? {} : { dispatch }),
    query: answering({
      compare_versions: { file_changes: changes },
      preview_restore: preview ?? { restore_preview: one(changes[0]?.path ?? "") },
    }),
  });
  await harness.openVersions();
  await harness.lookAt("cp2");
  await harness.click(harness.button("Compare with now"));
  return harness;
}

test("a changed document goes back alone: the dialog says the copy now is replaced and kept, and its Undo stands", async () => {
  h = await comparing([file("brief.txt"), file("notes.docx")], {
    dispatch: () => ({ version_restored: { checkpoint: "cp4" } }),
  });
  await h.click(h.one("version-restore-file"));
  expect(h.queried("preview_restore")).toEqual([
    { preview_restore: { matter: MATTER, checkpoint: "cp2", scope: ONE_FILE } },
  ]);
  expect(dialog()?.textContent).toContain("Restore '\u2068brief.txt\u2069' to how it was at");
  expect(describedBy()).toBe(
    "This replaces '\u2068brief.txt\u2069' as it is now. The copy you have now stays in Versions, so you can undo this.",
  );
  // the other document that differs is no part of it
  expect(dialog()?.textContent).not.toContain("notes.docx");

  await h.click(h.one("dialog-confirm"));
  expect(h.sent("restore_version")).toEqual([
    { matter: MATTER, checkpoint: "cp2", scope: ONE_FILE, thread: THREAD },
  ]);
  await h.deliver(MATTER_STREAM, [
    upsert(version(4, "restore")),
    upsert({
      marker: {
        order: 40,
        event: uuid(40, 9),
        ts: NOON + 4 * 60_000,
        kind: { version_restored: { from: "cp2", checkpoint: "cp4", scope: ONE_FILE } },
      },
    }),
  ]);
  expect(dialog()).toBeNull();
  const done = [...document.body.querySelectorAll(".ver-notice")].find((n) =>
    n.textContent?.includes("Restored"),
  );
  expect(done?.querySelector(".ver-notice-text")?.textContent).toMatch(
    /^Restored \u2068brief\.txt\u2069 to how it was at .+ today\.$/,
  );
  // the restore can be taken back, by restoring that document alone as it was just before it
  await h.click(done?.querySelector(".ver-restored-undo"));
  expect(h.queried("preview_restore").at(-1)).toEqual({
    preview_restore: { matter: MATTER, checkpoint: "cp3", scope: ONE_FILE, undoes: "cp4" },
  });
});

test("a removed document comes back, and one added since has no Restore this document", async () => {
  h = await comparing(
    [file("brief.txt", { change: "deleted" }), file("new.txt", { change: "added" })],
    // writing it adds it back
    {
      preview: {
        restore_preview: { ...one("brief.txt"), changes: [file("brief.txt", { change: "added" })] },
      },
    },
  );
  await h.click(h.one("version-restore-file"));
  expect(describedBy()).toBe(
    "'\u2068brief.txt\u2069' comes back as it was then. You can undo this.",
  );
  await h.click(h.button("Cancel"));
  expect(dialog()).toBeNull();
  await h.click(
    [...document.body.querySelectorAll(".ver-compare-bar button")].find((b) =>
      b.textContent?.includes("new.txt"),
    ),
  );
  expect(document.body.querySelector(".ver-pdf")?.getAttribute("aria-label")).toContain("new.txt");
  expect(h.one("version-restore-file")).toBeNull();
});

test("a file whose name is now a folder is refused in plain words, and nothing is restored", async () => {
  h = await comparing([file("brief.txt")], {
    preview: () => {
      throw { code: "matter.file_folder_clash", message: "clash", retryable: false };
    },
  });
  await h.click(h.one("version-restore-file"));
  expect(dialog()).toBeNull();
  expect(h.sent("restore_version")).toEqual([]);
  const words = errorWords("en", "matter.file_folder_clash");
  expect(words).toBe(
    "Clauseo can't restore this paper: a folder now has its name, or the folder it was in is now a file. Nothing has changed. Rename or move that folder or file, then try again.",
  );
  expect(document.body.textContent).toContain(words);
});

test("the file a compared document's Restore puts back: its path, or its old path when moved", () => {
  expect(restorable(file("a.docx"))).toBe("a.docx");
  expect(restorable(file("a.docx", { change: "deleted" }))).toBe("a.docx");
  expect(restorable(file("a.docx", { change: "added" }))).toBeNull();
  expect(restorable(file("b.docx", { change: { renamed: { from: "a.docx" } } }))).toBe("a.docx");
  expect(restorable(file("a.docx", { change: "copied" }))).toBeNull();
});

test("a second press while the preview is on its way asks once, and the dialog opens once", async () => {
  let answer: (v: unknown) => void = () => {};
  h = await comparing([file("brief.txt")], {
    preview: () => new Promise((resolve) => (answer = resolve)),
  });
  const button = h.one("version-restore-file");
  await h.click(button);
  await h.click(button);
  expect(h.queried("preview_restore")).toHaveLength(1);
  await act(async () => answer({ restore_preview: one("brief.txt") }));
  await h.settle();
  expect(document.body.querySelectorAll('[role="dialog"]')).toHaveLength(1);
});

test("Word holding the document: the dialog names it with Try again, which previews and restores that file alone", async () => {
  let held = true;
  const DOC = { files: { paths: ["notes.docx"] } };
  h = await comparing([file("notes.docx")], {
    preview: { restore_preview: { ...one("notes.docx"), locked: ["notes.docx"] } },
    dispatch: () => {
      if (held) throw { code: "matter.file_locked", message: "held", retryable: true };
      return { version_restored: { checkpoint: "cp4" } };
    },
  });
  await h.click(h.one("version-restore-file"));
  await h.click(h.one("dialog-confirm"));
  expect(dialog()?.textContent).toContain("Close '\u2068notes.docx\u2069' in Word to restore it.");
  expect(h.one("dialog-try-again")).not.toBeNull();
  held = false;
  await h.click(h.one("dialog-try-again"));
  expect(h.queried("preview_restore").at(-1)).toEqual({
    preview_restore: { matter: MATTER, checkpoint: "cp2", scope: DOC },
  });
  expect(h.sent("restore_version").map((c) => c.scope)).toEqual([DOC, DOC]);
  expect(dialog()).toBeNull();
});

test("a clash the restore itself meets is said in the dialog, with Try again, and nothing is restored", async () => {
  h = await comparing([file("brief.txt")], {
    dispatch: () => {
      throw { code: "matter.file_folder_clash", message: "clash", retryable: false };
    },
  });
  await h.click(h.one("version-restore-file"));
  await h.click(h.one("dialog-confirm"));
  expect(dialog()?.textContent).toContain(errorWords("en", "matter.file_folder_clash"));
  expect(h.one("dialog-try-again")).not.toBeNull();
  // said once: the band under the dialog doesn't say it again
  const band = [...document.body.querySelectorAll(".ver-room-band")].map((n) => n.textContent);
  expect(band.join(" ")).not.toContain(errorWords("en", "matter.file_folder_clash"));
});

test("the keyboard presses Restore this document; Cancel gives the focus back to it, and Restore to the done line's Undo", async () => {
  h = await comparing([file("brief.txt")], {
    dispatch: () => ({ version_restored: { checkpoint: "cp4" } }),
  });
  const button = h.one("version-restore-file");
  button?.focus();
  expect(document.activeElement).toBe(button);
  // Enter on a focused button presses it
  await act(async () => {
    button?.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
  });
  await h.click(button);
  expect(dialog()).not.toBeNull();
  expect(dialog()?.contains(document.activeElement)).toBe(true);
  await h.click(h.button("Cancel"));
  expect(dialog()).toBeNull();
  expect(document.activeElement).toBe(h.one("version-restore-file"));

  await h.click(h.one("version-restore-file"));
  await h.click(h.one("dialog-confirm"));
  await h.deliver(MATTER_STREAM, [
    upsert(version(4, "restore")),
    upsert({
      marker: {
        order: 40,
        event: uuid(40, 9),
        ts: NOON + 4 * 60_000,
        kind: { version_restored: { from: "cp2", checkpoint: "cp4", scope: ONE_FILE } },
      },
    }),
  ]);
  expect(dialog()).toBeNull();
  // the focus, lost with the dialog and its trigger, goes to the done line's Undo
  expect(document.activeElement).toBe(document.body.querySelector(".ver-restored-undo"));
});

test("Undo of a one-document restore says that document is back as it was", async () => {
  h = await comparing([file("brief.txt")], {
    dispatch: (c) =>
      c.restore_version?.undoes === undefined
        ? { version_restored: { checkpoint: "cp4" } }
        : { version_restored: { checkpoint: "cp5" } },
  });
  await h.click(h.one("version-restore-file"));
  await h.click(h.one("dialog-confirm"));
  await h.deliver(MATTER_STREAM, [
    upsert(version(4, "restore")),
    upsert({
      marker: {
        order: 40,
        event: uuid(40, 9),
        ts: NOON + 4 * 60_000,
        kind: { version_restored: { from: "cp2", checkpoint: "cp4", scope: ONE_FILE } },
      },
    }),
  ]);
  await h.click(document.body.querySelector(".ver-restored-undo"));
  await h.click(h.one("dialog-confirm"));
  // that document alone: an edit to another since the restore stays
  expect(h.queried("preview_restore").at(-1)).toEqual({
    preview_restore: { matter: MATTER, checkpoint: "cp3", scope: ONE_FILE, undoes: "cp4" },
  });
  expect(h.sent("restore_version").at(-1)).toEqual({
    matter: MATTER,
    checkpoint: "cp3",
    scope: ONE_FILE,
    undoes: "cp4",
    thread: THREAD,
  });
  expect(document.body.textContent).toMatch(
    /Undid the restore\. '\u2068brief\.txt\u2069' is back as it was before\./,
  );
});

test("a done line whose version is no longer in the timeline names the document alone", async () => {
  const { doneLine } = await import("./words.ts");
  const { freshSnapshot } = await import("./state.ts");
  const s = freshSnapshot(NOON, "UTC");
  const cal = { today: "2026-11-02", words: () => "" } as never;
  expect(
    doneLine("en", cal, s, {
      kind: "restoredFile",
      to: "cp9",
      file: "Drafts/brief.txt",
      outcome: { kind: "nothing" },
    }),
  ).toBe("Restored '\u2068brief.txt\u2069'.");
});

/** The restored marker for cp4, a restore of brief.txt alone from cp2. */
const MARKED = upsert({
  marker: {
    order: 40,
    event: uuid(40, 9),
    ts: NOON + 4 * 60_000,
    kind: { version_restored: { from: "cp2", checkpoint: "cp4", scope: ONE_FILE } },
  },
});
const doneText = () =>
  [...document.body.querySelectorAll(".ver-notice-text")].map((n) => n.textContent).join("\n");

test.each([
  ["already the same", { changes: [] }],
  ["kept by the host (over the cap, cloud-only, a link)", { changes: [], kept: ["brief.txt"] }],
])("a document %s holds Restore: the button isn't live, and no done line appears", async (_, p) => {
  h = await comparing([file("brief.txt")], {
    preview: { restore_preview: { ...one("brief.txt"), ...p } },
    dispatch: () => ({ version_restored: { checkpoint: "cp3" } }),
  });
  await h.click(h.one("version-restore-file"));
  expect(describedBy()).toMatch(/^Nothing will change\./);
  const confirm = h.one("dialog-confirm");
  expect(confirm?.getAttribute("aria-disabled")).toBe("true");
  await h.click(confirm);
  expect(h.sent("restore_version")).toEqual([]);
  expect(dialog()).not.toBeNull();
  expect(doneText()).not.toContain("Restored");
});

test("a document that changes while the dialog is open: asked again, Restore goes live, and the done line says so", async () => {
  let asked = 0;
  h = await comparing([file("brief.txt")], {
    preview: () => {
      asked += 1;
      return {
        restore_preview: asked === 1 ? { ...one("brief.txt"), changes: [] } : one("brief.txt"),
      };
    },
    dispatch: () => ({ version_restored: { checkpoint: "cp4" } }),
  });
  await h.click(h.one("version-restore-file"));
  expect(h.one("dialog-confirm")?.getAttribute("aria-disabled")).toBe("true");
  // the lawyer saves the document elsewhere: the matter moves, and the dialog asks again
  await h.deliver(MATTER_STREAM, [
    upsert(version(5, "outside_edits", { files: [file("brief.txt")] })),
  ]);
  expect(asked).toBe(2);
  expect(h.one("dialog-confirm")?.getAttribute("aria-disabled")).toBeNull();
  await h.click(h.one("dialog-confirm"));
  await h.deliver(MATTER_STREAM, [upsert(version(4, "restore")), MARKED]);
  expect(doneText()).toContain(
    "'\u2068brief.txt\u2069' had changed since you opened this, so Clauseo restored it as it was then. Your newer version is kept in Versions.",
  );
});

test.each([
  [
    "a folder now has its name",
    "brief.txt",
    "brief.txt",
    "Clauseo can't restore this paper: there's now a folder called '\u2068brief.txt\u2069' where it was. Nothing has changed. Rename or move that folder, then try again.",
  ],
  [
    "its folder is now a file",
    "Drafts/brief.txt",
    "Drafts",
    "Clauseo can't restore this paper: '\u2068Drafts\u2069', the folder it was in, is now a file. Nothing has changed. Rename or move that file, then try again.",
  ],
])(
  "a clash where %s names what's in the way, in the dialog and on the band",
  async (_, path, at, words) => {
    const clash = {
      code: "matter.file_folder_clash",
      message: "clash",
      retryable: false,
      files: [at],
    };
    h = await comparing([file(path)], {
      dispatch: () => {
        throw clash;
      },
    });
    await h.click(h.one("version-restore-file"));
    await h.click(h.one("dialog-confirm"));
    expect(dialog()?.textContent).toContain(words);
    await h.unmount();
    // found by the preview: the band says it, as no dialog opens
    h = await comparing([file(path)], {
      preview: () => {
        throw clash;
      },
    });
    await h.click(h.one("version-restore-file"));
    expect(dialog()).toBeNull();
    expect(document.body.textContent).toContain(words);
  },
);

test("a document renamed since comes back at its old name, and the dialog says the renamed copy stays", async () => {
  h = await comparing([file("brief, sent.txt", { change: { renamed: { from: "brief.txt" } } })], {
    preview: {
      restore_preview: { ...one("brief.txt"), changes: [file("brief.txt", { change: "added" })] },
    },
  });
  await h.click(h.one("version-restore-file"));
  expect(h.queried("preview_restore").at(-1)).toEqual({
    preview_restore: { matter: MATTER, checkpoint: "cp2", scope: ONE_FILE },
  });
  expect(describedBy()).toBe(
    "'\u2068brief.txt\u2069' comes back as it was then, next to '\u2068brief, sent.txt\u2069', which stays as it is. You can undo this.",
  );
});

test("an Undo of a one-document restore says what it did to the document, by its preview", async () => {
  const { doneLine } = await import("./words.ts");
  const { freshSnapshot } = await import("./state.ts");
  const s = freshSnapshot(NOON, "UTC");
  const cal = { today: "2026-11-02", words: () => "" } as never;
  const undo = (p: Partial<RestorePreview>) =>
    restoreDone(
      { id: "cp3", preview: { ...one("brief.txt"), ...p }, undoes: "cp4", file: "brief.txt" },
      { kind: "wrote", checkpoint: "cp5" },
      NOON,
    );
  const said = (p: Partial<RestorePreview>, platform: "mac" | "windows" = "mac") =>
    doneLine("en", cal, s, undo(p), platform);
  expect(said({})).toBe("Undid the restore. '\u2068brief.txt\u2069' is back as it was before.");
  expect(said({ changes: [], removes: ["brief.txt"] })).toBe(
    "Undid the restore. '\u2068brief.txt\u2069' wasn't in this matter before it, so Clauseo moved it to the Trash.",
  );
  expect(said({ changes: [], removes: ["brief.txt"] }, "windows")).toBe(
    "Undid the restore. '\u2068brief.txt\u2069' wasn't in this matter before it, so Clauseo moved it to the Recycle Bin.",
  );
  expect(said({ changes: [] })).toBe(
    "Nothing was undone: '\u2068brief.txt\u2069' has changed since the restore, so Clauseo left it as it is.",
  );
});
