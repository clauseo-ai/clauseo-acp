// fake_agent `tool-calls`: ten tool calls, one of each kind, each pending then in progress, nine
// completed and one (the fetch) failed. Every row shows its title and its last status.

import { expect } from "@playwright/test";
import { test } from "./fixture.ts";
import { expectTurnEnded, openMatter, sendPrompt } from "./flow.ts";
import { window } from "./selectors.ts";

test.use({ scenario: "tool-calls" });

/**
 * The scenario's tool calls: its id (a step name only), title, last status. Rows are found by
 * title, which no other title contains, so the test does not depend on which id the row carries.
 */
const CALLS = [
  ["tc-read-1", "Read brief.txt", "completed"],
  ["tc-edit-1", "Edit brief.txt", "completed"],
  ["tc-delete-1", "Delete old.txt", "completed"],
  ["tc-move-1", "Move notes/b.txt to archive/b.txt", "completed"],
  ["tc-search-1", 'Search for "indemnity"', "completed"],
  ["tc-execute-1", "Run the checks", "completed"],
  ["tc-think-1", "Weigh the cap against the fees", "completed"],
  ["tc-switch-1", "Switch to plan mode", "completed"],
  ["tc-fetch-1", "Fetch the terms page", "failed"],
  ["tc-other-1", "Ask the document service", "completed"],
] as const;

test("ten tool calls render with their titles and last status", async ({ app }) => {
  const w = window(app.page);
  await openMatter(w);
  await sendPrompt(w, "Review the brief");
  await expect(w.agentMessages.last()).toHaveText("Ran all ten tool calls.");
  await expectTurnEnded(w, "end_turn");
  await expect(w.toolCalls).toHaveCount(CALLS.length);
  for (const [id, title, status] of CALLS) {
    await test.step(id, async () => {
      const row = w.toolCallTitled(title);
      await expect(row).toHaveCount(1);
      await expect(row).toHaveAttribute("data-status", status);
    });
  }
});
