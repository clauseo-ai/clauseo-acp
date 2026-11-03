// The queue over the composer while Clauseo works (U7, matters/queue and held). fake_agent `queue-two-prompts`
// asks permission in prompt 1 and only then ends it; a second session/prompt before that response is a mismatch,
// so the second message must wait for the first turn's end. Enter queues the field's words while the turn waits on
// the permission: the message shows as a `queued` row with its turn, not in the transcript, and the permission is
// still pending. Allowing it ends the first turn, and only then does the queued message go: the row leaves the
// strip, the question shows in the transcript, and its answer arrives.
//
// The held promise (after Stop the next message waits until Send next) is not in `queue-two-prompts`: it has no
// cancel step, and a session/cancel while it waits for perm-1 is a mismatch. `send-now` plays the same stop: prompt 1
// streams, the client cancels, and prompt 2 ("Use the 2019 version instead") runs once the client sends it. So the
// held test is in its own block with that scenario.

import { expect } from "@playwright/test";
import { test } from "./fixture.ts";
import { expectTurnEnded, openMatter, sendPrompt } from "./flow.ts";
import { window } from "./selectors.ts";

test.use({ scenario: "claude-queue-two-prompts" });

/** Prompt 1: its title is the first conversation's. */
const FIRST = "Edit clause 4 for the new lease";
/** Prompt 2: exactly the text the scenario expects as prompt 2's only block. */
const SECOND = "Second question";

test("a message sent while Clauseo works queues, and goes once the answer ends", async ({
  app,
}) => {
  const w = window(app.page);
  await openMatter(w);
  await sendPrompt(w, FIRST);

  // Prompt 1 asks permission and waits for the answer: the card is pending and Stop shows.
  const card = w.permissions.first();
  await expect(card).toHaveAttribute("data-state", "pending");
  await expect(w.cancel).toBeVisible();

  // Enter queues the words: a `queued` row with its turn, the field empty, the transcript still one
  // message, and the permission still waiting (Enter with words never answers it).
  await w.composer.fill(SECOND);
  await w.composer.press("Enter");
  await expect(w.queued).toHaveCount(1);
  await expect(w.queued.first()).toContainText(SECOND);
  await expect(w.queued.first()).toHaveAttribute("data-turn", /.+/);
  await expect(w.queueSay).toContainText("1 queued");
  await expect(w.composer).toHaveValue("");
  await expect(w.userMessages).toHaveCount(1);
  await expect(card).toHaveAttribute("data-state", "pending");

  // Allow once: prompt 1 ends, and the queued message goes next. The row leaves the strip, its
  // question shows in the transcript, and prompt 2's answer streams.
  await w.permissionButton(card, "allow_once").click();
  await expect(card).toHaveAttribute("data-state", "resolved");
  await expect(w.agentMessages.first()).toHaveText("Clause 4 is fine.");
  await expect(w.turnStopped.first()).toHaveAttribute("data-stop-reason", "end_turn");
  await expect(w.queued).toHaveCount(0);
  await expect(w.userMessages).toHaveCount(2);
  await expect(w.userMessages.last()).toContainText(SECOND);
  await expect(w.agentMessages.last()).toHaveText("Second answer.");
  await expectTurnEnded(w, "end_turn");
});

test.describe("a stopped answer holds the queue", () => {
  test.use({ scenario: "claude-send-now" });

  test("after Stop the queue is held: the next message waits until Send next", async ({ app }) => {
    const w = window(app.page);
    await openMatter(w);
    await sendPrompt(w, "Summarise the indemnity clause");
    await expect(w.agentMessages.last()).toHaveText("The clause caps liability at");

    // Enter queues the second message while the answer streams.
    await w.composer.fill("Use the 2019 version instead");
    await w.composer.press("Enter");
    await expect(w.queued).toHaveCount(1);

    // Stop: the first turn ends as cancelled and the queue is held. The held line says nothing more
    // goes until the lawyer sends, Send next shows, and the message is still queued, not sent.
    await w.cancel.click();
    await expectTurnEnded(w, "cancelled");
    await expect(w.queueSay).toContainText("The queue is held.");
    await expect(w.queueSay).toContainText("nothing more is sent until you send.");
    await expect(w.sendNext).toBeVisible();
    await expect(w.queued).toHaveCount(1);
    await expect(w.userMessages).toHaveCount(1);

    // Send next: the message goes as the next turn, and its answer ends the turn.
    await w.sendNext.click();
    await expect(w.queued).toHaveCount(0);
    await expect(w.userMessages.last()).toContainText("Use the 2019 version instead");
    await expect(w.agentMessages.last()).toHaveText("Using the 2019 version.");
    await expectTurnEnded(w, "end_turn");
  });
});
