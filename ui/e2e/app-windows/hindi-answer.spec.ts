// fake_agent `claude-hindi-answer`: a question asked in Hindi, answered with Claude Haiku 4.5's recorded Hindi answer
// (the #137 format battery's p15-r1: a Devanagari heading, a table with bold row heads and ₹ amounts, a rule and a
// paragraph), streamed in 16 to 64 byte chunks, some ending on a virama so the next starts mid-conjunct; end_turn.
// The window shows the question and the whole answer: its text, NFC, is the recording's less the Markdown syntax, and
// the thread shows the Devanagari heading, the table's ₹ cells and the bold row heads (Hindi audit 5c).

import { readFileSync } from "node:fs";
import { expect } from "@playwright/test";
import { test } from "./fixture.ts";
import { expectTurnEnded, openMatter, sendPrompt } from "./flow.ts";
import { window } from "./selectors.ts";

test.use({ scenario: "claude-hindi-answer" });

/** The question, in Hindi: compare the limitation of liability clause in two draft supply agreements. */
const QUESTION = "दो मसौदा आपूर्ति समझौतों में दायित्व सीमा खंड की तुलना कीजिए।";

/** The recording, joined back from the scenario's chunks, so the spec and the scenario cannot drift apart. */
function recording(): string {
  const file = new URL(
    "../../../crates/fake_agent/scenarios/claude-hindi-answer.jsonl",
    import.meta.url,
  );
  type Line = { update?: { update?: { sessionUpdate?: string; content?: { text?: string } } } };
  return readFileSync(file, "utf8")
    .split("\n")
    .filter((l) => l.trim() !== "")
    .map((l) => JSON.parse(l) as Line)
    .flatMap((l) =>
      l.update?.update?.sessionUpdate === "agent_message_chunk"
        ? [l.update.update.content?.text ?? ""]
        : [],
    )
    .join("");
}

/** What a reader sees of Markdown, for comparing text: the table's delimiter row and the heading marks, bars and
 * asterisks dropped, and no whitespace, in NFC. The answer has no other syntax. */
function visible(markdown: string): string {
  return markdown
    .split("\n")
    .filter((l) => !/^\|?[\s:|-]+\|?$/.test(l))
    .join("\n")
    .replace(/^#+ /gm, "")
    .replace(/[*|]/g, "")
    .replace(/\s+/g, "")
    .normalize("NFC");
}

test("a Hindi answer streamed in chunks that split conjuncts arrives whole", async ({ app }) => {
  const w = window(app.page);
  const answer = recording();
  expect(answer).toMatch(/[\u0900-\u097F]/);
  await openMatter(w);
  await sendPrompt(w, QUESTION);
  await expectTurnEnded(w, "end_turn");

  // the question as asked, whole
  await expect(w.userMessages.last()).toContainText(QUESTION);

  // the answer's text, every code point, once the turn ended
  const message = w.agentMessages.last();
  const text = (await message.textContent()) ?? "";
  expect(text.replace(/\s+/g, "").normalize("NFC")).toBe(visible(answer));

  // the thread shows it as Devanagari: the heading, the bold row heads, the ₹ cells
  await expect(
    message.getByRole("heading", { name: "आपूर्ति समझौता: दायित्व सीमा खंड की तुलना" }),
  ).toBeVisible();
  await expect(message.locator("strong", { hasText: "प्रति घटना अधिकतम" })).toBeVisible();
  await expect(message.getByRole("cell", { name: "₹2,00,00,000" })).toBeVisible();
  await expect(message.getByRole("columnheader", { name: "सीमा (₹)" })).toBeVisible();
});
