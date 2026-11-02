// Claude's AskUserQuestion arrives as a form elicitation (elicitation-schema.ts, elicitation-form.tsx):
// the question is a pending `elicitation` row whose form holds a choice per question, and each
// question's `_custom` Other beneath it. fake_agent `ask-user-question` asks one question with
// options and an Other, and the spec answers it: one option picked, words in the Other, Send the
// answer; the row leaves pending and the turn ends normally. fake_agent `ask-user-question-stop`
// asks the same and waits; Stop the answer ends the turn as cancelled, as cancel-unanswered asserts.
//
// Both scenarios live on another team's branch (host-ask-and-mode) and are not on main yet, so
// the describe is skipped: it is collected, and unskipping it is the only change once they land.

import { expect } from "@playwright/test";
import { test } from "./fixture.ts";
import { expectTurnEnded, openMatter, sendPrompt } from "./flow.ts";
import { window } from "./selectors.ts";

test.describe("ask-user-question", () => {
  test.skip(
    true,
    "fake_agent's ask-user-question scenario is not on main yet (host-ask-and-mode); unskip once it lands",
  );

  test.describe("answered", () => {
    test.use({ scenario: "ask-user-question" });

    test("the form takes an option and the Other, and Send ends the turn", async ({ app }) => {
      const w = window(app.page);
      await openMatter(w);
      await sendPrompt(w, "Which court should the petition be filed in?");

      const row = app.page.getByTestId("elicitation");
      const form = app.page.getByTestId("elicitation-form");
      await expect(form).toBeVisible();
      await expect(row).toHaveAttribute("data-state", "pending");

      await app.page.getByTestId("elicitation-choice-question_0-0").click();
      await app.page.getByTestId("elicitation-input-question_0_custom").fill("Saket, with a note");
      await app.page.getByTestId("elicitation-send").click();

      // The answer is sent: the row leaves pending and its form goes with it.
      await expect(row).toHaveAttribute("data-state", "resolved");
      await expect(form).toHaveCount(0);
      await expectTurnEnded(w, "end_turn");
    });
  });

  test.describe("stopped", () => {
    test.use({ scenario: "ask-user-question-stop" });

    test("Stop the answer ends a turn waiting on the form as cancelled", async ({ app }) => {
      const w = window(app.page);
      await openMatter(w);
      await sendPrompt(w, "Which court should the petition be filed in?");

      const row = app.page.getByTestId("elicitation");
      const form = app.page.getByTestId("elicitation-form");
      await expect(form).toBeVisible();
      await expect(row).toHaveAttribute("data-state", "pending");

      await app.page.getByTestId("elicitation-stop").click();

      // Stop leaves the question unanswered: the row is no longer pending and its form is gone.
      await expect(row).not.toHaveAttribute("data-state", "pending");
      await expect(form).toHaveCount(0);
      await expectTurnEnded(w, "cancelled");
    });
  });
});
