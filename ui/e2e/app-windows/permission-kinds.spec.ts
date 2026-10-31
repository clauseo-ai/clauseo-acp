// fake_agent `permission-kinds`: four permission requests, each offering all four kinds; the
// scenario accepts only allow_once, allow_always, reject_once, reject_always in that order (any
// other answer ends fake_agent, so the closing answer never comes). allow_always passes Rust's
// native confirmation because the fixture sets E2E_CONFIRM_ALWAYS=accept.

import { expect } from "@playwright/test";
import { test } from "./fixture.ts";
import { expectTurnEnded, openMatter, sendPrompt } from "./flow.ts";
import { window, type OptionKind } from "./selectors.ts";

test.use({ scenario: "permission-kinds" });

const ANSWERS: readonly (readonly [string, OptionKind])[] = [
  ["Edit clause 1", "allow_once"],
  ["Edit clause 2", "allow_always"],
  ["Edit clause 3", "reject_once"],
  ["Edit clause 4", "reject_always"],
];

test("permission requests are approved and denied by option kind", async ({ app }) => {
  const w = window(app.page);
  await openMatter(w);
  await sendPrompt(w, "Edit the four clauses");
  for (const [title, kind] of ANSWERS) {
    await test.step(`${title}: ${kind}`, async () => {
      const card = w.permission(title);
      await expect(card).toBeVisible();
      // Every offered kind has its button.
      for (const offered of [
        "allow_once",
        "allow_always",
        "reject_once",
        "reject_always",
      ] as const) {
        await expect(w.permissionButton(card, offered)).toBeVisible();
      }
      await w.permissionButton(card, kind).click();
      // The card stays, answered, without buttons.
      await expect(card).toHaveAttribute("data-state", "resolved");
      await expect(w.permissionButton(card, kind)).toBeHidden();
    });
  }
  await expect(w.agentMessages.last()).toHaveText("Four edits were decided.");
  await expectTurnEnded(w, "end_turn");
  await expect(w.permissions).toHaveCount(ANSWERS.length);
});
