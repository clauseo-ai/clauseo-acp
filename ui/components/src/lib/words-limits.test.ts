import { expect, test } from "vitest";
import { clock } from "./words.ts";
import { lim } from "./words-limits.ts";
import { limitWords } from "./words-limits.hi.ts";
import { limitThreadWords } from "./words-limits-thread.hi.ts";
import { pc } from "./words-plan-card.ts";
import { planCardWords } from "./words-plan-card.hi.ts";

const ALL = { ...limitWords, ...limitThreadWords, ...planCardWords };

// The design's list of model names (Opus, Sonnet, Haiku, GPT-n), with the rest a provider labels: GPT, o1, o3, Codex-mini
// and Claude 3.
const MODEL_NAMES = /\b(Opus|Sonnet|Haiku|GPT|o1|o3|Codex-mini)\b|\bClaude 3/i;

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
    if (hi === null) continue; // hi: later, no Hindi yet
    expect(en).not.toMatch(
      /\bI\b|[\u2013\u2014]|\b(pencil|ink|step|git|token|session|terminal|safe)\b/i,
    );
    expect(`${en} ${hi}`).not.toMatch(MODEL_NAMES);
    expect(hi).not.toMatch(/पेंसिल|स्याही|चरण|कदम|(^|\s)मैं(\s|$)/);
    if (en.endsWith(".")) expect(hi.endsWith("।")).toBe(true);
  }
});

test("no limit string names a model in either language: a model's name only comes from the host (#236)", () => {
  const naming = Object.entries(ALL)
    .filter(([en, hi]) => MODEL_NAMES.test(`${en} ${hi}`))
    .map(([en]) => en);
  expect(naming).toEqual([]);
});

test("a per-model window's label takes the model as the host names it", () => {
  expect(pc("en", "{model} weekly limit", { model: "Fable" })).toBe("Fable weekly limit");
  expect(pc("hi", "{model} weekly limit", { model: "Fable" })).toBe("Fable की साप्ताहिक सीमा");
});
