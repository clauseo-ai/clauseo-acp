// fake_agent `exit-mid-turn`: one prompt, one answer chunk, then the agent's process exits 1
// mid-turn. The chunk stays in the transcript, the turn ends failed with agent.exited, the
// working line says the answer stopped (with no Restart until the host has it, H10), and the
// question is back in the field, so asking again starts a new turn. The relaunch's answer waits
// for fake_agent's one_of step (the fixme at the end).

import { expect } from "@playwright/test";
import { test } from "./fixture.ts";
import { expectTurnEnded, openMatter, sendPrompt } from "./flow.ts";
import { window } from "./selectors.ts";

const QUESTION = "Summarise the indemnity clause";
const CHUNK = "Starting on the answer.";

test.describe("on exit-mid-turn", () => {
  test.use({ scenario: "exit-mid-turn" });

  test("an agent that exits mid-answer leaves the answer so far, and asking again starts a new turn", async ({
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
    // stored session, and exit-mid-turn's handshake expects session/new only, so this checks the
    // new turn starts, not that it answers (the by-hand rows ask again with Claude Code itself).
    await w.send.click();
    await expect(w.userMessages).toHaveCount(2);
    await expect(w.userMessages.last()).toContainText(QUESTION);
    await expect(w.composer).toHaveValue("");
    await expect(w.error).toBeHidden();
    await expect(w.agentStopped).toBeHidden();
    await expect(w.agentMessages.first()).toHaveText(CHUNK);
  });
});

// The relaunch answering: needs fake_agent's one_of step (iris's branch fake-agent-one-of) and its
// scenario exit-mid-turn-then-load, whose second launch takes session/load and answers. Until then
// the relaunch fails on session/load and the turn neither runs nor ends (the host hang iris has),
// so Stop never shows: Windows x64 showed no Stop 15 s and 60 s after the second Ask.
test.describe("on exit-mid-turn-then-load", () => {
  test.use({ scenario: "exit-mid-turn-then-load" });

  test.fixme("asking again relaunches the agent, and the new turn runs and ends", async ({
    app,
  }) => {
    const w = window(app.page);
    await openMatter(w);
    await sendPrompt(w, QUESTION);
    await expectTurnEnded(w, "failed");
    await expect(w.composer).toHaveValue(QUESTION);
    await w.send.click();
    // the new turn is live: Stop stands in Ask's place
    await expect(w.cancel).toBeVisible();
    await expect(w.userMessages).toHaveCount(2);
    await expect(w.turnStopped).toHaveCount(2);
    await expect(w.agentMessages).toHaveCount(2);
  });
});
