// fake_agent `limit-rejected`: a turn at the usage limit (a rejected `_claude/rateLimit` with
// resetsAt 4102444800, in 2100 so it never passes, then end_turn). The window shows the stop and
// when the limit resets. The time is checked through `<time dateTime>`, so the visible format and
// the runner's time zone do not matter.

import { expect } from "@playwright/test";
import { test } from "./fixture.ts";
import { expectTurnEnded, openMatter, sendPrompt } from "./flow.ts";
import { window } from "./selectors.ts";

test.use({ scenario: "limit-rejected" });

/** The scenario's resetsAt, 4102444800 s. */
const RESETS_AT = new Date(4_102_444_800_000).toISOString();

test("a turn stopped at the limit shows the stop and its reset time", async ({ app }) => {
  const w = window(app.page);
  await openMatter(w);
  await sendPrompt(w, "Draft the clause");
  await expect(w.limitStop).toBeVisible();
  await expect(w.limitResetsAt).toHaveAttribute("datetime", RESETS_AT);
  await expect(w.limitResetsAt).not.toHaveText("");
  await expectTurnEnded(w, "at_limit");
});
