// Two conversations in one matter answer at the same time (issue 15, parallel answers). The first
// asks and its turn streams and waits. A second conversation in the same matter asks while it
// runs: its message goes at once, no wait card shows (that card is only for an undo, a restore or
// a move holding the matter), and its own answer streams while the first's still runs. Each row
// says what its own conversation does. Stopping one leaves the other answering.
//
// Every agent a test starts runs the same fake_agent scenario (scratch.ts writes one scenario into
// the settings), so `cancel-unanswered` plays in both conversations: each streams "The clause"
// and then waits for its cancel.

import { expect } from "@playwright/test";
import { test } from "./fixture.ts";
import { expectTurnEnded, newConversation, openMatter, sendPrompt, showRail } from "./flow.ts";
import { window, type Window } from "./selectors.ts";

test.use({ scenario: "claude-cancel-unanswered" });

/** 30 code points: the first conversation's own title. */
const FIRST = "Summarise the indemnity clause";
/** 28 code points: under 48, so the second conversation's title is its whole text. */
const SECOND = "Check the arbitration clause";
/** What cancel-unanswered streams before it waits for the cancel. */
const PARTIAL = "The clause";

/** Presses the row titled `title` and waits until it is the open conversation. */
async function openConversation(w: Window, title: string): Promise<void> {
  const row = w.conversationTitled(title);
  await showRail(w);
  await row.click();
  await expect(row).toHaveAttribute("aria-current", "true");
}

test("a second conversation answers while the first still runs", async ({ app }) => {
  const w = window(app.page);
  await openMatter(w);

  // A asks; its turn streams and waits.
  await sendPrompt(w, FIRST);
  await expect(w.agentMessages.last()).toHaveText(PARTIAL);
  await expect(w.cancel).toBeVisible();

  // B, a draft in the same matter, asks while A runs: the message goes at once, with no card.
  await newConversation(w);
  await expect(w.conversations).toHaveCount(2);
  await w.composer.fill(SECOND);
  await w.send.click();
  await expect(w.userMessages.last()).toContainText(SECOND);
  await expect(w.conversationTitled(SECOND)).toHaveAttribute("aria-current", "true");
  await expect(w.matterHold).toBeHidden();
  await expect(w.composer).toHaveValue("");

  // B's own answer streams while A's still runs; its working line says it answers.
  await expect(w.agentMessages.last()).toHaveText(PARTIAL);
  await expect(w.cancel).toBeVisible();
  await expect(w.working).toContainText("Answering.");
  await expect(w.matterHold).toBeHidden();

  // Each row says its own conversation answers: A's, read from B, and B's, read from A.
  await expect(w.conversationTitled(FIRST)).toHaveAttribute("data-state", "working");
  await openConversation(w, FIRST);
  await expect(w.conversationTitled(SECOND)).toHaveAttribute("data-state", "working");
  await expect(w.cancel).toBeVisible();

  // Stopping A ends A's turn only: A keeps the answer it streamed, and B still answers.
  await w.cancel.click();
  await expectTurnEnded(w, "cancelled");
  await expect(w.userMessages).toHaveCount(1);
  await expect(w.agentMessages.last()).toHaveText(PARTIAL);
  await expect(w.conversationTitled(SECOND)).toHaveAttribute("data-state", "working");

  // Clean up as cancel-unanswered does: stop B's turn.
  await openConversation(w, SECOND);
  await expect(w.agentMessages.last()).toHaveText(PARTIAL);
  await w.cancel.click();
  await expectTurnEnded(w, "cancelled");
  await expect(w.agentMessages.last()).toHaveText(PARTIAL);
});
