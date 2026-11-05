// Claude's AskUserQuestion arrives as a form elicitation (elicitation-schema.ts, elicitation-form.tsx):
// the question is a pending `elicitation` row whose form holds a choice per question, and each
// question's `_custom` Other beneath it. fake_agent `ask-user-question` asks two questions as
// claude-agent-acp 0.89.1 does (Law: a oneOf; Schedules: an anyOf array, each with its Other) and
// expects exactly question_0 "New York", question_1 ["Fees"] and question_1_custom "Exhibits", so
// the spec picks those: the row leaves pending and the turn ends normally. fake_agent
// `ask-user-question-stop` asks the same and waits; Stop the answer ends the turn as cancelled.

import { expect } from "@playwright/test";
import { test } from "./fixture.ts";
import { expectTurnEnded, openMatter, sendPrompt } from "./flow.ts";
import { window } from "./selectors.ts";

test.describe("ask-user-question", () => {
  test.describe("answered", () => {
    test.use({ scenario: "claude-ask-user-question" });

    test("the form takes an option, a box and an Other, and Send ends the turn", async ({
      app,
    }) => {
      const w = window(app.page);
      await openMatter(w);
      await sendPrompt(w, "Which court should the petition be filed in?");

      const row = app.page.getByTestId("elicitation");
      const form = app.page.getByTestId("elicitation-form");
      await expect(form).toBeVisible();
      await expect(row).toHaveAttribute("data-state", "pending");

      // Law: New York (the second option); Schedules: Fees (the first box), and Exhibits in its Other.
      // question_0_custom stays empty, so it is not sent.
      await app.page.getByTestId("elicitation-choice-question_0-1").click();
      await app.page.getByTestId("elicitation-choice-question_1-0").click();
      await app.page.getByTestId("elicitation-input-question_1_custom").fill("Exhibits");
      // The selectors are test ids, so the words on the buttons (Send, Don't answer) can change
      // without them. This spec checks only what the window shows; the shape of the answer the agent
      // received (question_0 the option's const, question_1 the boxes' consts, question_1_custom the words) is asserted by fake_agent's
      // ask-user-question scenario itself, which fails the turn otherwise.
      await app.page.getByTestId("elicitation-send").click();

      // The answer is sent: the row leaves pending and its form goes with it.
      await expect(row).toHaveAttribute("data-state", "resolved");
      await expect(form).toHaveCount(0);
      await expectTurnEnded(w, "end_turn");
    });
  });

  test.describe("stopped", () => {
    test.use({ scenario: "claude-ask-user-question-stop" });

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
