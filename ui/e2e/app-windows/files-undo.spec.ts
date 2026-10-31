// fake_agent `files-one-turn`: one turn creates notes/a.txt, edits brief.txt and deletes
// old.txt in the matter folder. Undo on that turn puts the folder back as it was.

import { expect } from "@playwright/test";
import { test } from "./fixture.ts";
import { expectTurnEnded, openMatter, sendPrompt } from "./flow.ts";
import { readTree } from "./scratch.ts";
import { window } from "./selectors.ts";

/** The scenario's description asks for these two files first. */
const BEFORE = { "brief.txt": "Clause 4 draft\n", "old.txt": "Old terms\n" };

test.use({ scenario: "files-one-turn", matterFiles: BEFORE });

test("Undo restores the files a turn changed", async ({ app, scratch }) => {
  const w = window(app.page);
  await openMatter(w);
  await sendPrompt(w, "Finalise clause 4");
  await expect(w.agentMessages.last()).toHaveText(
    "Created one note, edited the brief and deleted the old file.",
  );
  await expectTurnEnded(w, "end_turn");
  await expect(w.fileChanged).toHaveCount(3);
  expect(readTree(scratch.matterDir)).toEqual({
    "brief.txt": "Clause 4 final\n",
    "notes/a.txt": "Alpha\n",
  });

  await w.undo.last().click();
  await expect.poll(() => readTree(scratch.matterDir)).toEqual(BEFORE);
});
