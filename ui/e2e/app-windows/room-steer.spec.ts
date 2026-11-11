// Words typed while Clauseo answers, sent into the answer (U7, matters/steer-claude and steer-chatgpt). Three
// tests, each with its own fake_agent scenario:
//
// - steer-injected (claude-agent-acp, native steering): the steer hint shows once words are typed and says Enter adds
//   them to the answer; Enter sends `_session/steering` with the words (`delivery: "steer"`), the agent answers
//   injected and, as Claude Code does, writes them to its transcript, which is how the host confirms the steer. Only
//   then do the words stand in the answer as a steer mark; unconfirmed, they would come back into the field as not
//   sent. The answer then ends normally.
// - steer-fallback (codex-acp, steering without claudeCode): ChatGPT takes no words mid-answer, so Enter would queue
//   them; the hint's Send now (`delivery: "send_now"`, on every agent) cancels the answer, which keeps its text, and
//   sends the words as the next turn: a question of its own, no steer.
// - send-now (no steering): the hint says Enter sends the words when Claude finishes, and Enter queues them; a queued
//   message's Send now cancels prompt 1, keeps its text, and runs prompt 2 ("Use the 2019 version instead") to the
//   end.

import { expect } from "@playwright/test";
import { test } from "./fixture.ts";
import { expectTurnEnded, openMatter, sendPrompt } from "./flow.ts";
import { window } from "./selectors.ts";

/** The first question of each test: the conversation's title. */
const FIRST = "Summarise the indemnity clause";

test.describe("steer-injected", () => {
  test.use({ scenario: "claude-steer-injected" });

  test("words typed during an answer are steered into it, and the answer goes on", async ({
    app,
  }) => {
    const w = window(app.page);
    await openMatter(w);
    await sendPrompt(w, FIRST);
    // The answer streams its first chunk and waits for the steer: nothing typed, no hint.
    await expect(w.agentMessages.last()).toHaveText("The clause");
    await expect(w.steerHint).toBeHidden();

    // Words in the field raise the hint, which says Enter adds them to this answer.
    await w.composer.fill("Focus on clause 4");
    await expect(w.steerHint).toBeVisible();
    await expect(w.steerHintText).toContainText("Enter adds your words to this answer.");
    await expect(w.composer).toHaveValue("Focus on clause 4");

    // Enter steers them: the field clears, the hint goes, and once the host has read the words in Claude's
    // transcript they stand in the answer.
    await w.composer.press("Enter");
    await expect(w.composer).toHaveValue("");
    await expect(w.steerHint).toBeHidden();
    await expect(w.steerMark).toBeVisible();
    await expect(w.steerMark).toContainText("Focus on clause 4");

    // The agent took the steer in and finished its answer in the same turn: no second question, the words are not
    // back in the field, and nothing says they were not sent.
    await expect(w.agentMessages.last()).toContainText("caps liability at the fees paid.");
    await expectTurnEnded(w, "end_turn");
    await expect(w.steerMark).toContainText("Focus on clause 4");
    await expect(w.userMessages).toHaveCount(1);
    await expect(w.steerNotSent).toHaveCount(0);
    await expect(w.composer).toHaveValue("");
  });
});

test.describe("steer-fallback", () => {
  test.use({ scenario: "claude-prompt-stream", otherAgents: { "codex-acp": "steer-fallback" } });

  test("Send now on a ChatGPT answer keeps its text and runs the words next", async ({ app }) => {
    // steer-fallback launches as codex-acp with the shared codex handshake; its agent advertises steering
    // without promptQueueing, so its words never join an answer. Send now sends them with `delivery: "send_now"`:
    // the host cancels the answer, then sends the words as prompt 2.
    const w = window(app.page);
    await openMatter(w);
    await w.planOption("ChatGPT").click();
    await expect(w.planOption("ChatGPT")).toHaveAttribute("aria-pressed", "true");
    await sendPrompt(w, FIRST);
    await expect(w.agentMessages.last()).toHaveText("The clause");

    // ChatGPT's hint: words in the field, Enter sends them when ChatGPT finishes, Send now stops this answer and
    // sends them.
    await w.composer.fill("Focus on clause 4");
    await expect(w.steerHint).toBeVisible();
    await expect(w.steerHintText).toContainText("Enter sends your words when ChatGPT finishes.");
    await w.steerSendNow.click();
    await expect(w.composer).toHaveValue("");

    // The answer stops as cancelled and keeps its text; the words run as the next turn, a question of their own
    // after the first, and its answer ends the turn. A send now is no steer: no mark, no steer outcome line.
    await expect(w.turnStopped.first()).toHaveAttribute("data-stop-reason", "cancelled");
    await expect(w.agentMessages.first()).toHaveText("The clause");
    await expect(w.userMessages).toHaveCount(2);
    await expect(w.userMessages.first()).toContainText(FIRST);
    await expect(w.userMessages.last()).toContainText("Focus on clause 4");
    await expect(w.agentMessages.last()).toHaveText("Focusing on clause 4.");
    await expectTurnEnded(w, "end_turn");
    await expect(w.agentMessages.first()).toHaveText("The clause");
    await expect(w.steerMark).toHaveCount(0);
    await expect(w.steerOutcome("sent_now")).toHaveCount(0);
  });
});

test.describe("send-now", () => {
  test.use({ scenario: "claude-send-now" });

  test("Send now on a queued message keeps the answer so far and runs the words", async ({
    app,
  }) => {
    const w = window(app.page);
    await openMatter(w);
    await sendPrompt(w, FIRST);
    await expect(w.agentMessages.last()).toHaveText("The clause caps liability at");

    // This agent does not steer, so the hint says Enter sends the words when Claude finishes: Enter queues them,
    // and the queued row's Send now is the way to send them into the answer's place.
    await w.composer.fill("Use the 2019 version instead");
    await expect(w.steerHint).toBeVisible();
    await expect(w.steerHintText).toContainText("Enter sends your words when Claude finishes.");
    await w.composer.press("Enter");
    await expect(w.queued).toHaveCount(1);
    await w.queuedSendNow(w.queued.first()).click();

    // The first answer is cancelled and keeps its text; the queued words go next and end the turn.
    await expect(w.turnStopped.first()).toHaveAttribute("data-stop-reason", "cancelled");
    await expect(w.agentMessages.first()).toHaveText("The clause caps liability at");
    await expect(w.queued).toHaveCount(0);
    await expect(w.userMessages.last()).toContainText("Use the 2019 version instead");
    await expect(w.agentMessages.last()).toHaveText("Using the 2019 version.");
    await expectTurnEnded(w, "end_turn");
    await expect(w.agentMessages.first()).toHaveText("The clause caps liability at");
  });
});
