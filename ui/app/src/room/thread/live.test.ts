// A question's words from the view: one block is that block's own string, more are paragraphs; the entry's text
// stands in for a block the view doesn't hold.
import { expect, test } from "vitest";
import { textKey } from "../../store/view.ts";
import type { UserMessageEntry } from "../../transport/protocol.ts";
import { questionText } from "./live.ts";

const entry = (...texts: (string | null)[]): UserMessageEntry => ({
  order: 1,
  message: "m",
  turn: "t",
  content: texts.map((t) =>
    t === null ? { resource_link: { uri: "file:///a.pdf", name: "a.pdf" } } : { text: { text: t } },
  ),
  status: "sent",
  edited: false,
});

test("one text block reads straight from the view, or from the entry before the view has it", () => {
  const e = entry("Clause");
  const texts = new Map([[textKey("user_message:m", 0), "What does clause nine say?"]]);
  expect(questionText("user_message:m", e, texts)).toBe("What does clause nine say?");
  expect(questionText("user_message:m", e, undefined)).toBe("Clause");
});

test("several text blocks are paragraphs, skipping what isn't text", () => {
  const e = entry("First", null, "Second");
  const texts = new Map([[textKey("user_message:m", 2), "Second, in full"]]);
  expect(questionText("user_message:m", e, texts)).toBe("First\n\nSecond, in full");
  expect(questionText("user_message:m", entry(null), undefined)).toBe("");
});
