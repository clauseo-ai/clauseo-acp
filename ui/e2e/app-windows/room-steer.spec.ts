// Words typed while Clauseo answers, sent into the answer (U7, matters/steer-claude and steer-chatgpt). Three
// tests, each with its own fake_agent scenario:
//
// - steer-injected (claude-agent-acp, native steering): the steer hint shows once words are typed; Send now sends
//   `_session/steering` with the words, the agent answers injected, and the words stand in the answer as a steer
//   mark. The answer then ends normally.
// - steer-fallback (codex-acp, steering without claudeCode): Send now cancels the answer, which keeps its text, and
//   sends the words as the next turn, with a `sent_now` steer outcome. Skipped: see the block below.
// - send-now (no steering): no hint, since the agent cannot steer; a queued message's Send now cancels prompt 1,
//   keeps its text, and runs prompt 2 ("Use the 2019 version instead") to the end.

import { expect } from "@playwright/test";
import { test } from "./fixture.ts";
import { expectTurnEnded, openMatter, sendPrompt } from "./flow.ts";
import { window } from "./selectors.ts";

/** The first question of each test: the conversation's title. */
const FIRST = "Summarise the indemnity clause";

test.describe("steer-injected", () => {
  test.use({ scenario: "steer-injected" });

  test("words typed during an answer are steered into it, and the answer goes on", async ({
    app,
  }) => {
    const w = window(app.page);
    await openMatter(w);
    await sendPrompt(w, FIRST);
    // The answer streams its first chunk and waits for the steer: nothing typed, no hint.
    await expect(w.agentMessages.last()).toHaveText("The clause");
    await expect(w.steerHint).toBeHidden();

    // Words in the field raise the hint, which names no Send next and no queue of its own.
    await w.composer.fill("Focus on clause 4");
    await expect(w.steerHint).toBeVisible();
    await expect(w.composer).toHaveValue("Focus on clause 4");

    // Send now steers them: the field clears, the hint goes, and the words stand in the answer.
    await w.steerSendNow.click();
    await expect(w.composer).toHaveValue("");
    await expect(w.steerHint).toBeHidden();
    await expect(w.steerMark).toBeVisible();
    await expect(w.steerMark).toContainText("Focus on clause 4");

    // The agent injected the steer and finished its answer in the same turn.
    await expect(w.agentMessages.last()).toContainText("caps liability at the fees paid.");
    await expectTurnEnded(w, "end_turn");
    await expect(w.steerMark).toContainText("Focus on clause 4");
  });
});

test.describe("steer-fallback", () => {
  test.use({ scenario: "prompt-stream", otherAgents: { "codex-acp": "steer-fallback" } });

  test("Send now on a ChatGPT answer keeps its text and runs the words next", async ({ app }) => {
    // steer-fallback has no codex handshake, which the desktop app's codex-acp launch needs before
    // its first prompt: session/new with the ask mode among `modes` (crates/host/src/actor/turn/
    // session.rs, `on_started`: without it the launch is refused), `session/set_mode` (the host
    // sends it when the mode is not the ask mode), then `_codex/hooks/list` for the guard
    // (`prepare`). The scenario answers none of them, so as written it cannot run as codex-acp.
    // Its header says "codex-acp" and does not say how to configure it. Once the scenario includes
    // common/codex-handshake.jsonl (or fake_agent says otherwise), the steps below are the check.
    test.skip(
      true,
      "steer-fallback lacks codex-acp's launch handshake (modes, set_mode, _codex/hooks/list); the codex launch is refused without it",
    );

    const w = window(app.page);
    await openMatter(w);
    await w.planOption("ChatGPT").click();
    await expect(w.planOption("ChatGPT")).toHaveAttribute("aria-pressed", "true");
    await sendPrompt(w, FIRST);
    await expect(w.agentMessages.last()).toHaveText("The clause");

    // ChatGPT's hint: words in the field, Send now stops this answer and sends them first.
    await w.composer.fill("Focus on clause 4");
    await expect(w.steerHint).toBeVisible();
    await w.steerSendNow.click();

    // The answer stops as cancelled and keeps its text; the sent-now outcome shows; the words run as
    // the next turn, and its answer ends the turn.
    await expect(w.turnStopped.first()).toHaveAttribute("data-stop-reason", "cancelled");
    await expect(w.agentMessages.first()).toHaveText("The clause");
    await expect(w.steerOutcome("sent_now")).toBeVisible();
    await expect(w.userMessages.last()).toContainText("Focus on clause 4");
    await expect(w.agentMessages.last()).toHaveText("Focusing on clause 4.");
    await expectTurnEnded(w, "end_turn");
  });
});

test.describe("send-now", () => {
  test.use({ scenario: "send-now" });

  test("Send now on a queued message keeps the answer so far and runs the words", async ({
    app,
  }) => {
    const w = window(app.page);
    await openMatter(w);
    await sendPrompt(w, FIRST);
    await expect(w.agentMessages.last()).toHaveText("The clause caps liability at");

    // This agent does not steer, so typed words get no hint; Enter queues them, and the queued row's
    // Send now is the way to send them into the answer's place.
    await w.composer.fill("Use the 2019 version instead");
    await expect(w.steerHint).toBeHidden();
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
