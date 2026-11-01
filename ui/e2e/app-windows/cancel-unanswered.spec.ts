// fake_agent `cancel-unanswered`: the answer starts streaming, then the agent takes the
// session/cancel and never answers the prompt, so the host's 5 s fallback ends the turn. Cancel
// shows while the turn runs, and after Cancel the turn ends as cancelled. In the room
// (E2E_WINDOW=room) Stop stands in Ask's place, so Send is hidden during the turn; in R12's
// window Send stays and is disabled.

import { expect } from "@playwright/test";
import { test } from "./fixture.ts";
import { expectTurnEnded, openMatter, sendPrompt } from "./flow.ts";
import { window } from "./selectors.ts";

test.use({ scenario: "cancel-unanswered" });

test("Cancel ends a turn the agent never answers", async ({ app }) => {
  const w = window(app.page);
  await openMatter(w);
  await sendPrompt(w, "Summarise the indemnity clause");
  await expect(w.agentMessages.last()).toHaveText("The clause");
  await expect(w.cancel).toBeVisible();
  if (process.env["E2E_WINDOW"] === "room") await expect(w.send).toBeHidden();
  else await expect(w.send).toBeDisabled();
  await w.cancel.click();
  await expectTurnEnded(w, "cancelled");
  // The partial answer stays ("The answer so far is kept.").
  await expect(w.agentMessages.last()).toHaveText("The clause");
});
