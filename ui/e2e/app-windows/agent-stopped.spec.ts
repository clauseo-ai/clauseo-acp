// fake_agent `claude-exit-mid-turn`: one prompt, one answer chunk, then the agent's process exits 1
// mid-turn. The chunk stays in the transcript, the turn ends failed with agent.exited, the
// working line says the answer stopped (with no Restart until the host has it, H10), and the
// question is back in the field, so asking again starts a new turn. fake_agent
// `exit-mid-turn-then-load` plays the same first launch, and its relaunch takes session/load (which
// replays the chunk) and answers the asked-again question, so the second test sees that turn end.

import { expect } from "@playwright/test";
import { test } from "./fixture.ts";
import { expectTurnEnded, openMatter, sendPrompt } from "./flow.ts";
import { window } from "./selectors.ts";

const QUESTION = "Summarise the indemnity clause";
const CHUNK = "Starting on the answer.";
/** exit-mid-turn-then-load's answer to the question asked again. */
const SECOND = "Second answer.";

test.describe("on exit-mid-turn", () => {
  test.use({ scenario: "claude-exit-mid-turn" });

  // Hangs since 8c79cb832 (the first run on desktop's real setup wire, #494): asking again never
  // reaches a new turn and the test waits out its timeout, cancelling smoke (windows-2025). Fixme
  // until the app track's fix lands; the app-plan thread owns it.
  test.fixme("an agent that exits mid-answer leaves the answer so far, and asking again starts a new turn", async ({
    app,
  }) => {
    const w = window(app.page);
    await openMatter(w);
    await sendPrompt(w, QUESTION);
    await expectTurnEnded(w, "failed");
    await expect(w.turnStopped.last()).toHaveAttribute("data-failure", "agent.exited");
    await expect(w.agentMessages.last()).toHaveText(CHUNK);
    await expect(w.agentStopped).toBeVisible();
    await expect(w.agentStopped).toHaveAttribute("data-code", "agent.exited");
    await expect(w.restart).toBeHidden();
    // the stopped question is back in the empty field
    await expect(w.composer).toHaveValue(QUESTION);

    // asking again: send_prompt relaunches the agent and the host accepts the question as a new
    // turn. The scenario's second launch can't answer: the relaunch sends session/load for the
    // stored session, and exit-mid-turn's handshake expects session/new only, so that launch exits
    // at once and the new turn fails too, putting the question back in the field. How soon is the
    // runner's, so this checks only that the new turn starts; the field emptying, the stopped line
    // going and the answer are the next test's, on exit-mid-turn-then-load.
    await w.send.click();
    await expect(w.userMessages).toHaveCount(2);
    await expect(w.userMessages.last()).toContainText(QUESTION);
    await expect(w.agentMessages.first()).toHaveText(CHUNK);
  });
});

test.describe("on exit-mid-turn-then-load", () => {
  test.use({ scenario: "claude-exit-mid-turn-then-load" });

  test("asking again relaunches the agent, and the new turn answers and ends", async ({ app }) => {
    const w = window(app.page);
    await openMatter(w);
    await sendPrompt(w, QUESTION);
    await expectTurnEnded(w, "failed");
    await expect(w.turnStopped.last()).toHaveAttribute("data-failure", "agent.exited");
    await expect(w.agentMessages.last()).toHaveText(CHUNK);
    await expect(w.composer).toHaveValue(QUESTION);

    // asking again: send_prompt relaunches the agent, whose session/load replays the chunk. The host
    // drops agent text outside a turn, so the replay adds no row; the new turn answers and ends.
    await w.send.click();
    await expect(w.userMessages).toHaveCount(2);
    await expect(w.userMessages.last()).toContainText(QUESTION);
    await expect(w.turnStopped).toHaveCount(2);
    await expectTurnEnded(w, "end_turn");
    await expect(w.turnStopped.first()).toHaveAttribute("data-stop-reason", "failed");
    await expect(w.agentMessages).toHaveCount(2);
    await expect(w.agentMessages.first()).toHaveText(CHUNK);
    await expect(w.agentMessages.last()).toHaveText(SECOND);
    await expect(w.agentStopped).toBeHidden();
    await expect(w.error).toBeHidden();
    await expect(w.composer).toHaveValue("");
  });
});
