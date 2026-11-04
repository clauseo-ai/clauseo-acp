// @vitest-environment happy-dom
// U3's direct Undo meeting a lock, over FakeTransport: an undo refused with matter.file_locked names the Word documents
// the host names as held (one, or two or more in one line) in Word's lines under the change note; any other lock shows
// the error map's line; Try again, in Undo's
// place, sends the same undo_step with a new command_id; focus goes to Try again as the lines come and back to Undo,
// or the note, as they go; and axe finds nothing on the lock's lines.
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import axe from "axe-core";
import { act } from "react";
import { settled } from "@clauseo/components/lib/late.ts";
import { errorWords } from "@clauseo/components/lib/words-errors.ts";
import { th } from "@clauseo/components/lib/words-thread.ts";
import { vt } from "@clauseo/components/lib/words-versions.ts";
import { afterEach, beforeAll, expect, test } from "vitest";
import { reset, step, uuid } from "../../testing/stream-script.ts";
import type { FileChange, HostError, StepEntry, ViewEntry } from "../../transport/protocol.ts";
import { defaultOutcome, mountRoom, type Harness, type Script } from "../thread/harness.ts";

let h: Harness | null = null;
beforeAll(() => {
  // the page's own title, as index.html gives it, for axe's document-title rule
  const html = readFileSync(
    join(dirname(fileURLToPath(import.meta.url)), "../../../index.html"),
    "utf8",
  );
  document.title = /<title>([^<]*)<\/title>/.exec(html)?.[1] ?? "";
});
afterEach(async () => {
  await h?.unmount();
  h = null;
  document.body.replaceChildren();
});

const LOCKED: HostError = { code: "matter.file_locked", message: "held", retryable: true };

const file = (path: string): FileChange => ({
  path,
  change: "modified",
  size_bytes: 10,
  may_include_lawyer_edits: false,
});

const changed = (paths: string[], over: Partial<StepEntry> = {}): ViewEntry =>
  step(9, { files: paths.map(file), checkpoint_after: uuid(1, 9), ...over });

/** The room with one change note of `paths`, whose undo_step `refuse` answers while it returns an error. */
async function noteOf(
  paths: string[],
  refuse: () => HostError | null,
  script: Script = {},
): Promise<Harness> {
  const room = await mountRoom({
    ...script,
    dispatch: (command) => {
      if (command.undo_step !== undefined) {
        const error = refuse();
        if (error !== null) throw error;
      }
      return defaultOutcome(command);
    },
  });
  room.deliver([reset([changed(paths)])]);
  return room;
}

/** Presses a button and waits for the lock's chunk. */
async function press(room: Harness, testId: string): Promise<void> {
  await room.click(room.one(testId));
  await act(settled);
  await room.settle();
}

const lock = () => h!.one("word-lock");
const lines = () => [...(lock()?.querySelectorAll("p") ?? [])].map((p) => p.textContent);

test("an undo of one .docx refused with the lock names it in Word's lines, with Try again in Undo's place", async () => {
  h = await noteOf(["drafts/Reply.docx"], () => LOCKED);
  await press(h, "undo");
  expect(lock()?.getAttribute("role")).toBe("alert");
  expect(lines()).toEqual([
    "Close '\u2068Reply.docx\u2069' in Word to undo the change.",
    "Word holds the file while it is open. Nothing has changed yet.",
  ]);
  expect(h.one("word-lock-retry")?.textContent).toBe("Try again");
  // no Undo beside it, no error line, and no Cancel
  expect(h.one("undo")).toBeNull();
  expect(h.one("files-changed")?.querySelector('[data-testid="error"]')).toBeNull();
  expect(lock()?.querySelectorAll("button")).toHaveLength(1);
});

test("in Hindi the lines are Versions' own, with the file's name isolated", async () => {
  h = await noteOf(["Reply.docx"], () => LOCKED, { display: { language: "hi" } });
  await press(h, "undo");
  const file = "\u2068Reply.docx\u2069";
  expect(lines()).toEqual([
    vt("hi", "Close '{file}' in Word to undo the change.", { file }),
    vt("hi", "Word holds the file while it is open. Nothing has changed yet."),
  ]);
  expect(h.one("word-lock-retry")?.textContent).toBe(vt("hi", "Try again"));
});

test("an undo of three files refused naming one .docx names only that one, in Word's lines", async () => {
  h = await noteOf(["a/Lease.docx", "b/Notice.docx", "c/brief.txt"], () => ({
    ...LOCKED,
    files: ["a/Lease.docx"],
  }));
  await press(h, "undo");
  expect(lines()).toEqual([
    "Close '\u2068Lease.docx\u2069' in Word to undo the change.",
    "Word holds the file while it is open. Nothing has changed yet.",
  ]);
  expect(h.one("word-lock-retry")?.textContent).toBe("Try again");
});

test("an undo refused naming two Word documents names both, in one line of Word's", async () => {
  h = await noteOf(["a/Lease.docx", "b/Notice.docx", "c/brief.txt"], () => ({
    ...LOCKED,
    files: ["a/Lease.docx", "b/Notice.docx"],
  }));
  await press(h, "undo");
  expect(lines()).toEqual([
    "Close '\u2068Lease.docx\u2069' and '\u2068Notice.docx\u2069' in Word to undo the change.",
    "Word holds the file while it is open. Nothing has changed yet.",
  ]);
  expect(h.one("word-lock-retry")?.textContent).toBe("Try again");
  expect(h.one("undo")).toBeNull();
});

for (const [name, paths] of [
  ["one .txt", ["brief.txt"]],
  ["two files, one .txt", ["Notice.docx", "brief.txt"]],
] as const) {
  test(`an undo of ${name} refused with the lock shows the error map's line, with Try again`, async () => {
    h = await noteOf([...paths], () => LOCKED);
    await press(h, "undo");
    expect(lines()).toEqual([errorWords("en", "matter.file_locked")]);
    expect(lock()?.dataset["code"]).toBe("matter.file_locked");
    expect(lock()?.textContent).not.toContain("in Word");
    expect(h.one("word-lock-retry")).not.toBeNull();
    expect(h.one("undo")).toBeNull();
  });
}

test("Try again sends the same undo_step with a new command_id, waits while it is on its way, and the undo lands", async () => {
  let held = true;
  let release: () => void = () => {};
  const waiting = new Promise<void>((resolve) => (release = resolve));
  h = await mountRoom({
    dispatch: async (command) => {
      if (command.undo_step !== undefined) {
        if (held) throw LOCKED;
        await waiting;
      }
      return defaultOutcome(command);
    },
  });
  h.deliver([reset([changed(["Reply.docx"])])]);
  await press(h, "undo");
  held = false;
  await press(h, "word-lock-retry");
  // on its way: the block stays, and Try again waits, faint but focusable
  expect(lock()).not.toBeNull();
  expect(h.one("word-lock-retry")?.getAttribute("aria-disabled")).toBe("true");
  await press(h, "word-lock-retry");
  const undos = h.transport
    .only("dispatch")
    .map((c) => c.command)
    .filter((c) => c.command.undo_step !== undefined);
  expect(undos).toHaveLength(2);
  expect(undos[1]?.command).toEqual(undos[0]?.command);
  expect(undos[1]?.command_id).not.toBe(undos[0]?.command_id);
  await act(async () => release());
  await h.settle();
  expect(lock()).toBeNull();
  // the host's step says it is undone
  h.deliver([reset([changed(["Reply.docx"], { undone: true })])]);
  expect(h.one("files-changed")?.textContent).toContain(
    th("en", "You undid Clauseo's changes to 1 paper."),
  );
});

test("a Try again refused with the lock again keeps the lines; refused otherwise, the ordinary error line and Undo", async () => {
  let refusal: HostError = LOCKED;
  h = await noteOf(["Reply.docx"], () => refusal);
  await press(h, "undo");
  await press(h, "word-lock-retry");
  expect(lines()[0]).toBe("Close '\u2068Reply.docx\u2069' in Word to undo the change.");
  expect(h.sent("undo_step")).toHaveLength(2);
  refusal = { code: "store.io", message: "x", retryable: true };
  await press(h, "word-lock-retry");
  expect(lock()).toBeNull();
  const error = h.one("files-changed")?.querySelector<HTMLElement>('[data-testid="error"]');
  expect(error?.dataset["code"]).toBe("store.io");
  expect(error?.textContent).toBe(errorWords("en", "store.io"));
  expect(h.one("undo")).not.toBeNull();
});

for (const language of ["en", "hi"] as const) {
  test(`axe finds no violation on the lock's lines, Word's and the error map's (${language})`, async () => {
    const found: string[] = [];
    for (const paths of [["Reply.docx"], ["brief.txt"]]) {
      h = await noteOf(paths, () => LOCKED, { display: { language } });
      await press(h, "undo");
      expect(lock()).not.toBeNull();
      const results = await axe.run(document, { resultTypes: ["violations"] });
      found.push(
        ...results.violations.map(
          (v) => `${paths[0]}: ${v.id} at ${v.nodes.map((n) => n.target.join(" ")).join(", ")}`,
        ),
      );
      await h.unmount();
      h = null;
      document.body.replaceChildren();
    }
    expect(found).toEqual([]);
  }, 20_000);
}

/** Focuses a button and presses it, as the keyboard does. */
async function pressFocused(room: Harness, testId: string): Promise<void> {
  const button = room.one(testId);
  await act(async () => button?.focus());
  expect(document.activeElement).toBe(button);
  await press(room, testId);
}

test("focus: Undo pressed from the keyboard and refused with the lock goes to Try again, not the page", async () => {
  h = await noteOf(["Reply.docx"], () => LOCKED);
  await pressFocused(h, "undo");
  expect(h.one("undo")).toBeNull();
  expect(document.activeElement).toBe(h.one("word-lock-retry"));
});

test("focus: a Try again that lands goes back to Undo as the lines go", async () => {
  let held = true;
  h = await noteOf(["Reply.docx"], () => (held ? LOCKED : null));
  await pressFocused(h, "undo");
  held = false;
  await pressFocused(h, "word-lock-retry");
  expect(lock()).toBeNull();
  expect(h.one("undo")).not.toBeNull();
  expect(document.activeElement).toBe(h.one("undo"));
});

test("focus: a Try again refused otherwise goes back to Undo, beside the error line", async () => {
  let refusal: HostError = LOCKED;
  h = await noteOf(["Reply.docx"], () => refusal);
  await pressFocused(h, "undo");
  refusal = { code: "store.io", message: "x", retryable: true };
  await pressFocused(h, "word-lock-retry");
  expect(lock()).toBeNull();
  expect(h.one("files-changed")?.querySelector('[data-testid="error"]')).not.toBeNull();
  expect(document.activeElement).toBe(h.one("undo"));
});

test("focus: the host saying the changes are undone while the lines stand takes focus to the note", async () => {
  h = await noteOf(["Reply.docx"], () => LOCKED);
  await pressFocused(h, "undo");
  expect(document.activeElement).toBe(h.one("word-lock-retry"));
  h.deliver([reset([changed(["Reply.docx"], { undone: true })])]);
  await h.settle();
  expect(lock()).toBeNull();
  const note = h.one("files-changed")?.querySelector(".change-note");
  expect(note?.textContent).toContain(th("en", "You undid Clauseo's changes to 1 paper."));
  expect(document.activeElement).toBe(note);
});

test("focus: lines going while the lawyer is elsewhere leave focus where it is", async () => {
  let held = true;
  let release: () => void = () => {};
  const waiting = new Promise<void>((resolve) => (release = resolve));
  h = await mountRoom({
    dispatch: async (command) => {
      if (command.undo_step !== undefined) {
        if (held) throw LOCKED;
        await waiting;
      }
      return defaultOutcome(command);
    },
  });
  h.deliver([reset([changed(["Reply.docx"])])]);
  await pressFocused(h, "undo");
  held = false;
  await pressFocused(h, "word-lock-retry");
  // while the undo is on its way, the lawyer moves on
  const elsewhere = document.createElement("button");
  document.body.append(elsewhere);
  await act(async () => elsewhere.focus());
  await act(async () => release());
  await h.settle();
  expect(lock()).toBeNull();
  expect(document.activeElement).toBe(elsewhere);
});
