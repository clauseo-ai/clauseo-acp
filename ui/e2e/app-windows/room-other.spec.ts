// Two conversations in one matter, the second waiting behind the first's writer (U7). The first
// asks and its turn runs. In the second, Ask raises the wait panel (`other-wait`) and sends
// nothing; the panel names the first conversation by its title and shows no clock time. Its Queue
// sends, and the turn waits as `Queued` while the first holds the matter's writer: the panel keeps
// Stop the other alone, without its heading, and the working line says the wait. Stop the other (`stop-other`) cancels
// the first turn, so the second turn starts. The first keeps the answer it streamed before the
// cancel.
//
// Every agent a test starts runs the same fake_agent scenario (scratch.ts writes one scenario into
// the settings), so `cancel-unanswered` plays in both conversations. Each thread launches its own
// agent, and the second agent gets its `session/prompt` only once the writer passes to its turn
// (crates/host/src/actor/turn/steps.rs, `on_launched`: the turn queues after its session is
// attached). So B's answer starts after `stop-other` and A's turn ends.

import { expect } from "@playwright/test";
import { test } from "./fixture.ts";
import { expectTurnEnded, newConversation, openMatter, sendPrompt, showRail } from "./flow.ts";
import { window, type Window } from "./selectors.ts";

test.use({ scenario: "cancel-unanswered" });

/** 30 code points: the first conversation's own title. */
const FIRST = "Summarise the indemnity clause";
/** 28 code points: under 48, so the second conversation's title is its whole text. */
const SECOND = "Check the arbitration clause";
/** What cancel-unanswered streams before it waits for the cancel. */
const PARTIAL = "The clause";
/** A wall-clock time such as 14:05; the wait panel never shows one. */
const CLOCK = /\d{1,2}:\d{2}/;

/** Presses the row titled `title` and waits until it is the open conversation. */
async function openConversation(w: Window, title: string): Promise<void> {
  const row = w.conversationTitled(title);
  await showRail(w);
  await row.click();
  await expect(row).toHaveAttribute("aria-current", "true");
}

test("a second conversation waits behind the first's writer until its Stop", async ({ app }) => {
  const w = window(app.page);
  await openMatter(w);

  // A asks; its turn streams and waits, so it holds the matter's writer.
  await sendPrompt(w, FIRST);
  await expect(w.agentMessages.last()).toHaveText(PARTIAL);
  await expect(w.cancel).toBeVisible();
  await expect(w.send).toBeHidden();

  // B, a draft in the same matter: no panel over its empty field.
  await newConversation(w);
  await expect(w.conversations).toHaveCount(2);
  await expect(w.otherWait).toBeHidden();

  // B's Ask raises the panel, which names A and has Queue; nothing is sent and the words stay.
  await w.composer.fill(SECOND);
  await w.send.click();
  await expect(w.otherWait).toBeVisible();
  await expect(w.otherWait).toContainText(FIRST);
  await expect(w.queue(w.otherWait)).toBeVisible();
  await expect(w.composer).toHaveValue(SECOND);
  await expect(w.userMessages).toHaveCount(0);

  // Queue sends. B's turn waits: the panel stays with Stop the other alone, focused, without its
  // heading and with no clock time; the working line names A, and B's agent has not been given
  // its prompt, so it has no answer yet.
  await w.queue(w.otherWait).click();
  await expect(w.userMessages.last()).toContainText(SECOND);
  await expect(w.conversationTitled(SECOND)).toHaveAttribute("aria-current", "true");
  await expect(w.otherWait).toBeVisible();
  await expect(w.queue(w.otherWait)).toHaveCount(0);
  await expect(w.stopOther(w.otherWait)).toBeFocused();
  await expect(w.otherWait).not.toContainText(FIRST);
  expect((await w.otherWait.textContent()) ?? "").not.toMatch(CLOCK);
  await expect(w.working).toContainText(`Waiting for '${FIRST}' to finish.`);
  await expect(w.agentMessages).toHaveCount(0);

  // The open conversation's own row says nothing of its state (the design's rail); read from A,
  // B's row waits for A, by A's title. Back in B, its wait is still there.
  await openConversation(w, FIRST);
  await expect(w.conversationTitled(SECOND)).toHaveAttribute("data-state", "waiting");
  await expect(w.conversationTitled(SECOND)).toHaveAttribute(
    "title",
    new RegExp(`Waiting for '${FIRST}'`),
  );
  await openConversation(w, SECOND);
  await expect(w.otherWait).toBeVisible();

  // Stop the other conversation's turn: A's turn is cancelled, the panel goes, and B's turn
  // starts, so its answer streams in B.
  await w.stopOther(w.otherWait).click();
  await expect(w.otherWait).toBeHidden();
  await expect(w.agentMessages.last()).toHaveText(PARTIAL);
  await expect(w.cancel).toBeVisible();

  // A keeps the answer it had streamed, and its turn ended as cancelled.
  await openConversation(w, FIRST);
  await expect(w.userMessages).toHaveCount(1);
  await expect(w.userMessages).toContainText(FIRST);
  await expect(w.agentMessages.last()).toHaveText(PARTIAL);
  await expect(w.turnStopped.last()).toHaveAttribute("data-stop-reason", "cancelled");

  // Clean up as cancel-unanswered does: stop B's turn.
  await openConversation(w, SECOND);
  await expect(w.agentMessages.last()).toHaveText(PARTIAL);
  await w.cancel.click();
  await expectTurnEnded(w, "cancelled");
  await expect(w.agentMessages.last()).toHaveText(PARTIAL);
});
