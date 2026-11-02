// A conversation asked with ChatGPT (U6, R15's done-when 3, left open after U5 replaced R12's window). With two
// agents set up, `agents.claude-acp` (prompt-stream) and `agents.codex-acp` (fake_agent `codex-handshake`, whose
// session the host first sets to workspace-write), a new conversation's status line offers the plan to ask
// with, Claude or ChatGPT. Picking ChatGPT makes its first question's thread run Codex: the answer streams in, the
// status line names the ChatGPT plan, and the conversation's row names it too.
//
// The rows name a plan only where it differs from the matter's, and the matter's plan is the one it uses most, a tie
// going to the one used most recently (agent.ts's matterPlan). The Claude conversation is asked first and the
// ChatGPT one second, so the tie goes to ChatGPT: the matter's plan is ChatGPT, the Claude row names "Claude", and
// the ChatGPT row names no plan, since it is the matter's own. The rows expose no agent id, so the answer's text (only
// codex-handshake says it) is what shows the thread ran as codex-acp.

import { expect } from "@playwright/test";
import { test } from "./fixture.ts";
import { expectTurnEnded, newConversation, openMatter, sendPrompt } from "./flow.ts";
import { window } from "./selectors.ts";

test.use({ scenario: "prompt-stream", otherAgents: { "codex-acp": "codex-handshake" } });

/** The Claude conversation's question: its title, 30 code points. */
const FIRST = "Summarise the indemnity clause";
/** prompt-stream's answer. */
const CLAUDE_ANSWER = "The clause caps liability at the fees paid in the last twelve months.";
/** The ChatGPT conversation's question: its title, 14 code points. */
const SECOND = "Are you ready?";
/** codex-handshake's answer, in two chunks. */
const CODEX_ANSWER = "Codex here. Ready to work on the matter.";

test("a conversation asked with ChatGPT is answered by Codex and named ChatGPT", async ({
  app,
}) => {
  const w = window(app.page);
  await openMatter(w);
  // The matter's first conversation asks with Claude, the default.
  await sendPrompt(w, FIRST);
  await expect(w.agentMessages.last()).toHaveText(CLAUDE_ANSWER);
  await expectTurnEnded(w, "end_turn");

  // A new conversation offers the plan to ask with, Claude pressed (the matter's plan), until its first question.
  await newConversation(w);
  await expect(w.planOption("Claude")).toHaveAttribute("aria-pressed", "true");
  await expect(w.planOption("ChatGPT")).toHaveAttribute("aria-pressed", "false");
  await w.planOption("ChatGPT").click();
  await expect(w.planOption("ChatGPT")).toHaveAttribute("aria-pressed", "true");
  await expect(w.planOption("Claude")).toHaveAttribute("aria-pressed", "false");

  // The first question creates the thread with the chosen agent: codex-handshake streams its answer.
  await sendPrompt(w, SECOND);
  await expect(w.agentMessages.last()).toHaveText(CODEX_ANSWER);
  await expectTurnEnded(w, "end_turn");

  // Once the question has fixed the plan the choice goes, and the status line names the plan.
  await expect(w.planChoice).toHaveCount(0);
  await expect(w.statusPlan).toHaveText("Uses your ChatGPT plan");

  // Two conversations: the Claude one names its plan, the ChatGPT one (the matter's plan) names none.
  await expect(w.conversations).toHaveCount(2);
  await expect(w.conversationPlan(w.conversationRow(FIRST))).toHaveText("Claude");
  await expect(w.conversationPlan(w.conversationRow(SECOND))).toHaveCount(0);
});
