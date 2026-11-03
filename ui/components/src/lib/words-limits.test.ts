import { expect, test } from "vitest";
import { clock } from "./words.ts";
import { lim, limitWords } from "./words-limits.ts";
import { limitThreadWords } from "./words-limits-thread.ts";
import { planCardWords } from "./words-plan-card.ts";

const ALL = { ...limitWords, ...limitThreadWords, ...planCardWords };

// The design's list of model names (Opus, Sonnet, Haiku, GPT-n), with the rest a provider labels: GPT, o1, o3, Codex-mini
// and Claude 3.
const MODEL_NAMES = /\b(Opus|Sonnet|Haiku|GPT|o1|o3|Codex-mini)\b|\bClaude 3/i;
// The one exception: the two window labels the provider gives its model windows, which Rohan rules on (U12-14, U12-15).
// Only these two keys may name a model, and both must.
const MODEL_RULED: readonly string[] = ["Opus weekly limit", "Sonnet weekly limit"];

test("the limit's words say until when, the part of the day first in Hindi, and that no reset was given", () => {
  expect(lim("en", "Limit reached until {time}", { time: clock("en", "9:40 pm") })).toBe(
    "Limit reached until 9:40 pm",
  );
  expect(lim("hi", "Limit reached until {time}", { time: clock("hi", "9:40 pm") })).toBe(
    "रात 9:40 बजे तक सीमा पूरी",
  );
  expect(lim("hi", "Limit reached. Reset not given.")).toBe("सीमा पूरी। रीसेट का समय नहीं बताया गया।");
});

test("no limit string says I or a banned word, and Hindi sentences end in the danda", () => {
  for (const [en, hi] of Object.entries(ALL)) {
    expect(en).not.toMatch(
      /\bI\b|[\u2013\u2014]|\b(pencil|ink|step|git|token|session|terminal|safe)\b/i,
    );
    if (!MODEL_RULED.includes(en)) expect(`${en} ${hi}`).not.toMatch(MODEL_NAMES);
    expect(hi).not.toMatch(/पेंसिल|स्याही|चरण|कदम|(^|\s)मैं(\s|$)/);
    if (en.endsWith(".")) expect(hi.endsWith("।")).toBe(true);
  }
});

test("only the two window labels Rohan rules on name a model, in either language", () => {
  const naming = Object.entries(ALL)
    .filter(([en, hi]) => MODEL_NAMES.test(`${en} ${hi}`))
    .map(([en]) => en);
  expect(naming.sort()).toEqual([...MODEL_RULED].sort());
});
