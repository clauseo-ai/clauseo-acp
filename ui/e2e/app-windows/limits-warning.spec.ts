// fake_agent `limit-warning`: a turn whose usage_update carries claude-agent-acp's rate-limit
// warning (`allowed_warning`, `five_hour`, utilization 0.96), then end_turn. The warning is not a
// stop: the plan line shows the near tone, the composer's notice line shows the warning, and Ask
// stays enabled. The turn ends normally, so no stop line or stop mark shows.

import { expect } from "@playwright/test";
import { test } from "./fixture.ts";
import { expectTurnEnded, openMatter, sendPrompt } from "./flow.ts";
import { window } from "./selectors.ts";

test.use({ scenario: "limit-warning" });

test("a near-limit warning shows on the plan line and under the field, and sending stays open", async ({
  app,
}) => {
  const w = window(app.page);
  await openMatter(w);
  // The start-up read runs in the background. A live signal that arrives while the first read is
  // still in flight is dropped (the model's Loading rule), so the warning waits for the first
  // answer: the plan line leaves `loading`.
  await expect(w.plan).not.toHaveAttribute("data-freshness", "loading");
  await sendPrompt(w, "Check the last clause");
  await expectTurnEnded(w, "end_turn");
  await expect(w.plan).toHaveAttribute("data-tone", "near");
  await expect(w.limitWarning).toBeVisible();
  // Ask is enabled by words in the field (composer-field.tsx), never by the limit.
  await w.composer.fill("And the next clause");
  await expect(w.send).toBeEnabled();
});
