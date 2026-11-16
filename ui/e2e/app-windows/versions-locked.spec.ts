// fake_agent `claude-files-one-turn`, as versions.spec.ts. Word holds brief.txt open: the file is
// open, shared for reading only, and its owner file `~$brief.txt` sits beside it (word-hold.ts;
// since #409 an owner file alone is stale). Restore to the import version is refused and writes
// nothing, and the dialog says a paper is open in another program and offers Try again. Once Word
// lets go of the file and the owner file is gone, Try again restores the matter.

import { existsSync } from "node:fs";
import { join } from "node:path";
import { expect } from "@playwright/test";
import { test } from "./fixture.ts";
import {
  FILES_SCENARIO,
  expectTurnEnded,
  finishFirstRead,
  openMatter,
  sendPrompt,
} from "./flow.ts";
import { readTree } from "./scratch.ts";
import { window } from "./selectors.ts";

/** The scenario's description asks for these two files first. */
const BEFORE = { "brief.txt": "Clause 4 draft\n", "old.txt": "Old terms\n" };
/** The matter after the turn. */
const AFTER = { "brief.txt": "Clause 4 final\n", "notes/a.txt": "Alpha\n" };
/** The matter after restoring the import version: a restore never deletes, so notes/a.txt stays. */
const RESTORED = { ...BEFORE, "notes/a.txt": "Alpha\n" };
/**
 * The restore dialog's line for a file added since the version (versions/words.ts). The words put
 * each file name between Unicode isolates (U+2068, U+2069), so the name is matched with them.
 */
const STAYS_LINE = /'\u2068?a\.txt\u2069?', added since, stays\./;
/** Word's owner file for brief.txt: `~$` and the whole name (locks.rs `owner_names`). */
const OWNER = "~$brief.txt";
const OWNER_BYTES = "owner file: open in Word";
/** matter.file_locked's line (words-errors.ts). */
const LOCKED_LINE = "A paper is open in another program. Close it, then try again.";

test.use({ scenario: FILES_SCENARIO, matterFiles: BEFORE });

test("Restore waits while Word holds a file, then lands once it is closed", async ({
  app,
  scratch,
  wordLock,
}) => {
  const w = window(app.page);
  await openMatter(w);
  await finishFirstRead(w);
  await sendPrompt(w, "Finalise clause 4");
  await expect(w.agentMessages.last()).toHaveText(
    "Created one note, edited the brief and deleted the old file.",
  );
  await expectTurnEnded(w, "end_turn");
  await expect(w.fileChanged).toHaveCount(3);
  expect(readTree(scratch.matterDir)).toEqual(AFTER);

  // Word opens brief.txt: it holds the file, sharing reading only, and its owner file appears
  // beside it.
  const held = await wordLock.hold({
    document: "brief.txt",
    owner: OWNER,
    ownerBytes: OWNER_BYTES,
  });

  await w.versionsButton.click();
  await expect(w.versionsPanel).toBeVisible();
  await w.version("import").click();
  await w.versionRestore.click();
  // A restore never deletes (crates/matters/src/restore.rs): the note the turn added stays.
  await expect(w.dialog).toContainText(STAYS_LINE);
  await w.dialogConfirm.click();

  // Refused: the dialog shows the lock line and offers Try again.
  await expect(w.dialogTryAgain).toBeVisible();
  await expect(w.dialog).toContainText(LOCKED_LINE);
  expect(existsSync(join(scratch.matterDir, OWNER))).toBe(true);

  // Closed in Word: the file is let go and the owner file goes. Nothing moved while it was held,
  // and Try again goes through.
  await held.release();
  expect(readTree(scratch.matterDir)).toEqual(AFTER);
  await w.dialogTryAgain.click();
  await expect(w.dialog).toBeHidden();
  await expect.poll(() => readTree(scratch.matterDir)).toEqual(RESTORED);

  // Below 1200 px (the e2e window is 1028 px wide) picking a version closed the panel; the
  // toolbar's Versions brings it back. One restore version: the refused try kept none.
  if (!(await w.versionsPanel.isVisible())) {
    await w.versionsButton.click();
  }
  await expect(w.version("restore")).toHaveCount(1);
});
