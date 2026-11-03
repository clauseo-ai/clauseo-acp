// fake_agent `limit-warning`: a turn whose usage_update carries claude-agent-acp's rate-limit
// warning (`allowed_warning`, `five_hour`, utilization 0.96), then end_turn. The warning is not a
// stop: the composer's notice line shows the warning, and Ask stays enabled. The turn ends normally,
// so no stop line or stop mark shows.
//
// Done-when 2's plan-tone check (the plan line's `data-tone` "near") waits for QU.8's words for
// LiveOnly. With fake_agent registered as claude-acp the host's usage reader fails, the view goes
// live_only, and lookOf hides the plan line ("no line" until QU.8's words, plan-model.ts). So this
// spec asserts the warning shows and the plan line is absent, not its tone, until those words land.

import { expect } from "@playwright/test";
import { test } from "./fixture.ts";
import { expectTurnEnded, openMatter, sendPrompt } from "./flow.ts";
import { window } from "./selectors.ts";

test.use({ scenario: "limit-warning" });

test("a near-limit warning shows under the field, and sending stays open", async ({ app }) => {
  const w = window(app.page);
  await openMatter(w);
  // The start-up read runs in the background, and the plan line shows `loading` while it runs
  // (live-plan.tsx renders the plan with data-freshness "loading"). A live signal that arrives
  // during that read is dropped (the model's Loading rule), so the send waits until the line has
  // left: the read is over and the view is live_only, which has no line.
  await expect(w.plan).toHaveCount(0);
  await sendPrompt(w, "Check the last clause");
  await expectTurnEnded(w, "end_turn");
  // the warning carries the near state while the plan line stays hidden (LiveOnly, see the header)
  await expect(w.limitWarning).toBeVisible();
  await expect(w.plan).toHaveCount(0);
  // Ask is enabled by words in the field (composer-field.tsx), never by the limit.
  await w.composer.fill("And the next clause");
  await expect(w.send).toBeEnabled();
});
