// fake_agent `files-one-turn`: one turn creates notes/a.txt, edits brief.txt and deletes old.txt in
// the matter folder. Versions lists the turn's version (`after_turn`) with its paper; Undo from that
// version's band, confirmed in the dialog, puts the folder back as it was and adds an `undo` version.
// While the panel is open the rail of matters stands out of the page; closing the panel brings it back.
// The toolbar's Undo, with the panel open, opens the same turn's undo dialog.

import { expect } from "@playwright/test";
import { test } from "./fixture.ts";
import { expectTurnEnded, openMatter, sendPrompt } from "./flow.ts";
import { readTree } from "./scratch.ts";
import { window, type Window } from "./selectors.ts";

/** The scenario's description asks for these two files first. */
const BEFORE = { "brief.txt": "Clause 4 draft\n", "old.txt": "Old terms\n" };

test.use({ scenario: "files-one-turn", matterFiles: BEFORE });

/** Opens the matter and plays the scenario's one turn. */
async function playTurn(w: Window): Promise<void> {
  await openMatter(w);
  await sendPrompt(w, "Finalise clause 4");
  await expect(w.agentMessages.last()).toHaveText(
    "Created one note, edited the brief and deleted the old file.",
  );
  await expectTurnEnded(w, "end_turn");
  await expect(w.fileChanged).toHaveCount(3);
}

test("Undo from Versions puts back the files a turn changed", async ({ app, scratch }) => {
  const w = window(app.page);
  await playTurn(w);

  await w.versionsButton.click();
  await expect(w.versionsPanel).toBeVisible();
  await expect(w.rail).toBeHidden();

  const turn = w.version("after_turn");
  await expect(turn).toHaveCount(1);
  await expect(turn).toContainText("brief.txt");
  await turn.click();
  await expect(w.versionUndo).toBeVisible();
  await w.versionUndo.click();
  await w.dialogConfirm.click();
  await expect(w.dialog).toBeHidden();

  await expect(w.version("undo")).toHaveCount(1);
  await expect.poll(() => readTree(scratch.matterDir)).toEqual(BEFORE);

  await w.versionsClose.click();
  await expect(w.versionsPanel).toBeHidden();
  await expect(w.rail).toBeVisible();
});

test("The toolbar's Undo opens the newest turn's undo dialog over Versions", async ({
  app,
  scratch,
}) => {
  const w = window(app.page);
  await playTurn(w);
  await w.versionsButton.click();
  await expect(w.versionsPanel).toBeVisible();

  await w.toolbarUndo.click();
  await expect(w.dialog).toContainText("Undo the change Clauseo made at");
  await w.dialogConfirm.click();
  await expect(w.dialog).toBeHidden();
  await expect(w.version("undo")).toHaveCount(1);
  await expect.poll(() => readTree(scratch.matterDir)).toEqual(BEFORE);
});
