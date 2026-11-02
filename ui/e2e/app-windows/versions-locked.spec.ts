// fake_agent `files-one-turn`, as versions.spec.ts. Word holds brief.txt open: its owner file
// `~$brief.txt` sits beside it. Restore to the import version is refused and writes nothing, and the
// dialog says which file holds it and offers Try again. Once the owner file is gone, Try again
// restores the matter.

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
/** The matter after restoring the import version: a restore never deletes, so notes/a.txt stays. */
const RESTORED = { ...BEFORE, "notes/a.txt": "Alpha\n" };
/**
 * The restore dialog's line for a file added since the version (versions/words.ts). The words put
 * each file name between Unicode isolates (U+2068, U+2069), so the name is matched with them.
 */
const STAYS_LINE = /'\u2068?a\.txt\u2069?', added since, stays\./;
/** The refusal's line for the file Word holds (restore-dialog.tsx), isolates as above. */
const HELD_LINE = /Close '\u2068?brief\.txt\u2069?' in Word to restore it\./;
/** Word's owner file for brief.txt: `~$` and the whole name (locks.rs `owner_names`). */
const OWNER = "~$brief.txt";
const OWNER_BYTES = "owner file: open in Word";
/** matter.file_locked's line (words-errors.ts). */
const LOCKED_LINE = "A paper is open in another program. Close it, then try again.";

test.use({ scenario: "files-one-turn", matterFiles: BEFORE });

test("Restore waits while Word holds a file, then lands once it is closed", async ({
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

  await w.versionsButton.click();
  await expect(w.versionsPanel).toBeVisible();
  await w.version("import").click();
  await w.versionRestore.click();
  // A restore never deletes (crates/matters/src/restore.rs): the note the turn added stays.
  await expect(w.dialog).toContainText(STAYS_LINE);
  await w.dialogConfirm.click();

  // Refused: the dialog names the file, offers Try again, and the lock line shows; nothing moved.
  await expect(w.dialogTryAgain).toBeVisible();
  await expect(w.dialog).toContainText(HELD_LINE);
  await expect(app.page.getByText(LOCKED_LINE)).toBeVisible();
  expect(readFileSync(brief, "utf8")).toBe(AFTER["brief.txt"]);
  expect(readTree(scratch.matterDir)).toEqual({ ...AFTER, [OWNER]: OWNER_BYTES });

  // Closed in Word: Try again goes through.
  rmSync(join(scratch.matterDir, OWNER));
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
