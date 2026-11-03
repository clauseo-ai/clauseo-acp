import { expect, test } from "vitest";
import { clock } from "./words.ts";
import { lim, limitWords } from "./words-limits.ts";
import { limitThreadWords } from "./words-limits-thread.ts";
import { planCardWords } from "./words-plan-card.ts";

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
  for (const [en, hi] of Object.entries({ ...limitWords, ...limitThreadWords, ...planCardWords })) {
    expect(en).not.toMatch(
      /\bI\b|[\u2013\u2014]|\b(pencil|ink|step|git|token|session|terminal|safe)\b/i,
    );
    expect(hi).not.toMatch(/पेंसिल|स्याही|चरण|कदम|(^|\s)मैं(\s|$)/);
    if (en.endsWith(".")) expect(hi.endsWith("।")).toBe(true);
  }
});
