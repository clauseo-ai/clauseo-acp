// fake_agent `claude-tool-calls`: ten tool calls, one of each kind, each pending then in progress, nine
// completed and one (the fetch) failed. They fold into one step group whose line keeps the failure in
// view; opened, every row says what it did in the lawyer's words, a paper by its file name, with its
// last status.

import { expect } from "@playwright/test";
import { test } from "./fixture.ts";
import { expectTurnEnded, openMatter, sendPrompt } from "./flow.ts";
import { window } from "./selectors.ts";

test.use({ scenario: "claude-tool-calls" });

/**
 * The scenario's tool calls: its id (a step name only), the row's words, last status. Rows are found
 * by their words, which no other row's contain, so the test does not depend on which id the row
 * carries.
 */
const CALLS = [
  ["tc-read-1", "Read brief.txt.", "completed"],
  ["tc-edit-1", "Changed brief.txt.", "completed"],
  ["tc-delete-1", "Deleted old.txt.", "completed"],
  ["tc-move-1", "Moved b.txt.", "completed"],
  ["tc-search-1", 'Search for "indemnity".', "completed"],
  ["tc-execute-1", "Ran a task.", "completed"],
  ["tc-think-1", "Thought it through.", "completed"],
  ["tc-switch-1", "Switch to plan mode.", "completed"],
  ["tc-fetch-1", "Tried to read terms.html online.", "failed"],
  ["tc-other-1", "Ask the document service.", "completed"],
] as const;

test("ten tool calls fold into one step group, which opens to their rows and last status", async ({
  app,
}) => {
  const w = window(app.page);
  await openMatter(w);
  await sendPrompt(w, "Review the brief");
  await expect(w.agentMessages.last()).toHaveText("Ran all ten tool calls.");
  await expectTurnEnded(w, "end_turn");
  // folded: one line, the failure kept on it, and no rows until it is opened
  await expect(w.stepGroupLines).toHaveCount(1);
  await expect(w.stepGroupLines).toContainText("1 failed.");
  await expect(w.toolCalls).toHaveCount(0);
  await w.stepGroupLines.click();
  await expect(w.stepGroupLines).toHaveAttribute("aria-expanded", "true");
  await expect(w.toolCalls).toHaveCount(CALLS.length);
  for (const [id, words, status] of CALLS) {
    await test.step(id, async () => {
      const row = w.toolCallTitled(words);
      await expect(row).toHaveCount(1);
      await expect(row).toHaveAttribute("data-status", status);
    });
  }
});
