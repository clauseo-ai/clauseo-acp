// fake_agent `prompt-stream`: one prompt, three thought chunks and five answer chunks, end_turn.
// The window shows the whole thought and the whole answer, and the turn's end.

import { expect } from "@playwright/test";
import { test } from "./fixture.ts";
import { expectTurnEnded, openMatter, sendPrompt } from "./flow.ts";
import { window } from "./selectors.ts";

test.use({ scenario: "prompt-stream" });

test("a streamed answer and its thought render in full", async ({ app }) => {
  const w = window(app.page);
  await openMatter(w);
  await sendPrompt(w, "Summarise the indemnity clause");
  await expect(w.agentMessages.last()).toHaveText(
    "The clause caps liability at the fees paid in the last twelve months.",
  );
  await expect(w.thoughts.last()).toHaveText("Reading the indemnity clause first.");
  await expectTurnEnded(w, "end_turn");
});
