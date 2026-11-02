// Two conversations in one matter (U6). The first is asked and answered; New conversation opens a
// draft that says "New conversation" with an empty transcript; its first send creates the thread,
// and the room names it from that message: a message over 48 code points takes its first 6 words.
// Pressing either row then shows that conversation's own transcript.
//
// Each thread launches its own agent (crates/host/src/actor/turn/agents.rs), so fake_agent's
// `prompt-stream` plays from the start in both, with the same answer: the transcripts are told
// apart by the user's question.

import { expect } from "@playwright/test";
import { test } from "./fixture.ts";
import { expectTurnEnded, newConversation, openMatter, sendPrompt, showRail } from "./flow.ts";
import { window, type Window } from "./selectors.ts";

test.use({ scenario: "prompt-stream" });

/** 30 code points: its own title. */
const FIRST = "Summarise the indemnity clause";
/** 83 code points: over 48, so the title is its first 6 words. */
const SECOND =
  "Check whether the arbitration clause survives termination of the services agreement";
const SECOND_TITLE = "Check whether the arbitration clause survives";
const ANSWER = "The clause caps liability at the fees paid in the last twelve months.";

/** Asks `question` and waits for prompt-stream's answer and end_turn. */
async function ask(w: Window, question: string): Promise<void> {
  await sendPrompt(w, question);
  await expect(w.agentMessages.last()).toHaveText(ANSWER);
  await expectTurnEnded(w, "end_turn");
}

/** Presses the row titled `title`; the transcript then holds `shown` and not `hidden`. Below 1200 px the rail is
 * folded behind Matters (a conversation started or pressed folds it), so it is raised first. */
async function openRow(w: Window, title: string, shown: string, hidden: string): Promise<void> {
  const row = w.conversationRow(title);
  await showRail(w);
  await row.click();
  await expect(row).toHaveAttribute("aria-current", "true");
  await expect(w.userMessages).toHaveCount(1);
  await expect(w.userMessages).toContainText(shown);
  await expect(w.transcript).not.toContainText(hidden);
  await expect(w.agentMessages.last()).toHaveText(ANSWER);
}

test("two conversations in a matter, the second titled from its first message", async ({ app }) => {
  const w = window(app.page);
  await openMatter(w);
  await ask(w, FIRST);
  await expect(w.conversationRow(FIRST)).toHaveAttribute("aria-current", "true");

  // The draft: a row of its own, current, with an empty transcript; the first stays listed.
  await newConversation(w);
  await expect(w.conversations).toHaveCount(2);
  await expect(w.conversationRow(FIRST)).not.toHaveAttribute("aria-current", "true");

  await ask(w, SECOND);
  const second = w.conversationRow(SECOND_TITLE);
  await expect(second).toHaveAttribute("aria-current", "true");
  // Exactly the 6 words: the title text holds nothing of the seventh word on.
  await expect(w.conversationTitle(second)).toHaveText(SECOND_TITLE);
  await expect(w.conversationRow("New conversation")).toHaveCount(0);
  await expect(w.conversations).toHaveCount(2);

  await openRow(w, FIRST, FIRST, SECOND);
  await openRow(w, SECOND_TITLE, SECOND, FIRST);
});
