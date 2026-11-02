// fake_agent `files-one-turn`, as files-undo.spec.ts. Word holds brief.txt open: its owner file
// `~$brief.txt` sits beside it. Undo on that turn writes three files, so it is refused whole and
// writes nothing; the room shows the lock block with Try again. Once the owner file is gone, Try
// again puts the folder back as it was.

import { readFileSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { expect } from "@playwright/test";
import { test } from "./fixture.ts";
import { expectTurnEnded, openMatter, sendPrompt } from "./flow.ts";
import { readTree } from "./scratch.ts";
import { window } from "./selectors.ts";

/** The scenario's description asks for these two files first. */
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

test.use({ scenario: "files-one-turn", matterFiles: BEFORE });

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

  // Refused: the lock block shows with Try again, and nothing moved.
  await expect(w.wordLock).toBeVisible();
  await expect(w.wordLock).toContainText(LOCKED_LINE);
  await expect(w.wordLockRetry).toBeVisible();
  expect(readFileSync(brief, "utf8")).toBe(AFTER["brief.txt"]);
  expect(readTree(scratch.matterDir)).toEqual({ ...AFTER, [OWNER]: OWNER_BYTES });

  // Closed in Word: Try again goes through, and the lock block goes.
  rmSync(join(scratch.matterDir, OWNER));
  await w.wordLockRetry.click();
  await expect(w.wordLock).toBeHidden();
  await expect.poll(() => readTree(scratch.matterDir)).toEqual(BEFORE);
});
