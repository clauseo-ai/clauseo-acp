// Two turns' Undo refused while Word holds a file, then landed once it is closed. Each test plays
// its own fake_agent scenario:
//
// - `files-one-turn`: three files change, so the lock block shows the general line. Word holds
//   brief.txt open: its owner file `~$brief.txt` sits beside it. Undo on that turn writes three
//   files, so it is refused whole and writes nothing; the room shows the lock block with Try again.
//   Once the owner file is gone, Try again puts the folder back as it was.
// - `word-one-docx` (scenarios/word-one-docx.jsonl): one turn changes one `.docx`, Petition.docx, so
//   the lock block names that Word document. Word holds it open: its owner file `~$tition.docx`
//   sits beside it.

import { readFileSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { expect } from "@playwright/test";
import { test } from "./fixture.ts";
import { expectTurnEnded, openMatter, sendPrompt } from "./flow.ts";
import { readTree } from "./scratch.ts";
import { window } from "./selectors.ts";

/** The `files-one-turn` scenario's description asks for these two files first. */
const BEFORE = { "brief.txt": "Clause 4 draft\n", "old.txt": "Old terms\n" };
/** The matter after the turn. */
const AFTER = { "brief.txt": "Clause 4 final\n", "notes/a.txt": "Alpha\n" };
/** Word's owner file for brief.txt: `~$` and the whole name (locks.rs `owner_names`). */
const OWNER = "~$brief.txt";
const OWNER_BYTES = "owner file: open in Word";
/**
 * matter.file_locked's line (words-errors.ts). The lock block shows the general line, not the one
 * that names Word: the undo writes three files, so no single file is the one held.
 */
const LOCKED_LINE = "A paper is open in another program. Close it, then try again.";

/** The one-`.docx` scenario, read from its file: a path, so fake_agent loads it as one. */
const WORD_SCENARIO = fileURLToPath(new URL("scenarios/word-one-docx.jsonl", import.meta.url));
/** The scenario's description asks for this file first. */
const PETITION_BEFORE = { "Petition.docx": "Petition draft\n" };
/** The matter after the turn. */
const PETITION_AFTER = { "Petition.docx": "Petition final\n" };
/**
 * Word's owner file for Petition.docx: `~$` and the name without its first two characters, the
 * rule Word follows for an eight-character stem (locks.rs `owner_names`, which checks this and the
 * two shorter forms too).
 */
const PETITION_OWNER = "~$tition.docx";
/** The lock block's Word lines (word-lock.tsx). The words put the file name between isolates. */
const WORD_LINE = /Close '\u2068?Petition\.docx\u2069?' in Word to undo the change\./;
const WORD_NOTE = "Word holds the file while it is open. Nothing has changed yet.";

test.describe("files-one-turn", () => {
  test.use({ scenario: "claude-files-one-turn", matterFiles: BEFORE });

  test("Undo waits while Word holds a file, then lands once it is closed", async ({
    app,
    scratch,
  }) => {
    const w = window(app.page);
    const brief = join(scratch.matterDir, "brief.txt");
    await openMatter(w);
    await sendPrompt(w, "Finalise clause 4");
    await expect(w.agentMessages.last()).toHaveText(
      "Created one note, edited the brief and deleted the old file.",
    );
    await expectTurnEnded(w, "end_turn");
    await expect(w.fileChanged).toHaveCount(3);
    expect(readTree(scratch.matterDir)).toEqual(AFTER);

    // Word opens brief.txt: its owner file appears beside it.
    writeFileSync(join(scratch.matterDir, OWNER), OWNER_BYTES);

    await w.undo.last().click();

    // Refused: the lock block shows with Try again, Undo is gone, and nothing moved.
    await expect(w.wordLock).toBeVisible();
    await expect(w.wordLock).toContainText(LOCKED_LINE);
    await expect(w.wordLockRetry).toBeVisible();
    await expect(w.undo).toHaveCount(0);
    expect(readFileSync(brief, "utf8")).toBe(AFTER["brief.txt"]);
    expect(readTree(scratch.matterDir)).toEqual({ ...AFTER, [OWNER]: OWNER_BYTES });

    // Closed in Word: Try again goes through, and the lock block goes.
    rmSync(join(scratch.matterDir, OWNER));
    await w.wordLockRetry.click();
    await expect(w.wordLock).toBeHidden();
    await expect.poll(() => readTree(scratch.matterDir)).toEqual(BEFORE);
  });
});

test.describe("word-one-docx", () => {
  test.use({ scenario: WORD_SCENARIO, matterFiles: PETITION_BEFORE });

  test("Undo of one Word document waits while Word holds it, then lands once it is closed", async ({
    app,
    scratch,
  }) => {
    const w = window(app.page);
    const petition = join(scratch.matterDir, "Petition.docx");
    await openMatter(w);
    await sendPrompt(w, "Finalise the petition");
    await expect(w.agentMessages.last()).toHaveText("Edited the petition.");
    await expectTurnEnded(w, "end_turn");
    await expect(w.fileChanged).toHaveCount(1);
    expect(readTree(scratch.matterDir)).toEqual(PETITION_AFTER);

    // Word opens Petition.docx: its owner file appears beside it.
    writeFileSync(join(scratch.matterDir, PETITION_OWNER), OWNER_BYTES);

    await w.undo.last().click();

    // Refused: the lock block names the Word document, with Try again; Undo is gone; nothing moved.
    await expect(w.wordLock).toBeVisible();
    await expect(w.wordLock).toContainText(WORD_LINE);
    await expect(w.wordLock).toContainText(WORD_NOTE);
    await expect(w.wordLockRetry).toBeVisible();
    await expect(w.undo).toHaveCount(0);
    expect(readFileSync(petition, "utf8")).toBe(PETITION_AFTER["Petition.docx"]);
    expect(readTree(scratch.matterDir)).toEqual({
      ...PETITION_AFTER,
      [PETITION_OWNER]: OWNER_BYTES,
    });

    // Closed in Word: Try again goes through, and the lock block goes.
    rmSync(join(scratch.matterDir, PETITION_OWNER));
    await w.wordLockRetry.click();
    await expect(w.wordLock).toBeHidden();
    await expect.poll(() => readTree(scratch.matterDir)).toEqual(PETITION_BEFORE);
  });
});
