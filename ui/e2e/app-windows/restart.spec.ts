// fake_agent `prompt-stream` for one turn, then the app is closed through its window and started
// again on the same data folder: it reopens the newest-created matter and its newest thread,
// and the turn shows as it did. No agent is needed to show it, so the second launch
// sends fake_agent nothing.

import { expect } from "@playwright/test";
import { test } from "./fixture.ts";
import { expectTurnEnded, openMatter, sendPrompt } from "./flow.ts";
import { window } from "./selectors.ts";

test.use({ scenario: "prompt-stream" });

const PROMPT = "Summarise the indemnity clause";
const ANSWER = "The clause caps liability at the fees paid in the last twelve months.";

test("a restart brings the thread back", async ({ app, apps }) => {
  const first = window(app.page);
  await openMatter(first);
  await sendPrompt(first, PROMPT);
  await expect(first.agentMessages.last()).toHaveText(ANSWER);
  await expectTurnEnded(first, "end_turn");
  await apps.stop(app, "close");

  const again = await apps.start();
  const w = window(again.page);
  await expect(w.matter).toHaveText("Matter");
  await expect(w.userMessages.last()).toContainText(PROMPT);
  await expect(w.agentMessages.last()).toHaveText(ANSWER);
  await expect(w.thoughts.last()).toHaveText("Reading the indemnity clause first.");
  await expectTurnEnded(w, "end_turn");
});
