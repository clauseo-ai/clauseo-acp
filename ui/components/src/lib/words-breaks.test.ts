// U10's words: a stream that failed says so before its code's sentence, and each start-up code has its own line, with
// Hindi that ends in the danda and no banned word.
import { expect, test } from "vitest";
import { breakWords, brk, streamFailedWords } from "./words-breaks.ts";
import { ERROR_WORDS, errorWords, type ErrorCode } from "./words-errors.ts";
import { STARTUP_WORDS, startupWordList, startupWords, type StartupCode } from "./words-startup.ts";
import { clock } from "./words.ts";

const BANNED_EN = /\bI\b|[\u2013\u2014]|\b(pencil|ink|step|git|token|session|terminal|safe)\b/i;
const BANNED_HI = /पेंसिल|स्याही|चरण|कदम|(^|\s)मैं(\s|$)/;

test("a stream that failed says the thread stopped coming in, then its code's sentence, in both languages", () => {
  expect(streamFailedWords("en", "store.io", true)).toBe(
    "This conversation stopped updating in this window. Clauseo couldn't read or write its data. Try again.",
  );
  expect(streamFailedWords("hi", "store.io", true)).toBe(
    `यह बातचीत इस विंडो में अपडेट होना बंद हो गई। ${errorWords("hi", "store.io")}`,
  );
  expect(streamFailedWords("en", "store.melted", false)).toContain(errorWords("en", "internal"));
});

test("with no Try again, a stream that failed never says to try again; with it, the code's own sentence", () => {
  expect(streamFailedWords("en", "store.disk_full", false)).toBe(
    "This conversation stopped updating in this window. The computer's disk is full. Free some space.",
  );
  expect(streamFailedWords("hi", "store.disk_full", false)).toBe(
    "यह बातचीत इस विंडो में अपडेट होना बंद हो गई। कंप्यूटर की डिस्क भर गई है। कुछ जगह खाली करें।",
  );
  expect(streamFailedWords("en", "store.disk_full", true)).toContain("then try again.");
  // no code that isn't retryable on the wire says "try again" once its stream fails
  const notRetryable: ErrorCode[] = [
    "store.disk_full",
    "store.corrupt",
    "store.newer_version",
    "internal",
    "unknown",
  ];
  for (const code of notRetryable)
    expect(streamFailedWords("en", code, false).toLowerCase()).not.toContain("try again");
});

test("a stopped mark on another day names it: on {day} at {time}", () => {
  expect(
    brk("en", "The answer stopped here on {day} at {time}", {
      day: "Monday",
      time: clock("en", "4:31 pm"),
    }),
  ).toBe("The answer stopped here on Monday at 4:31 pm");
  expect(
    brk("hi", "The answer stopped here on {day} at {time}", {
      day: "सोमवार",
      time: clock("hi", "4:31 pm"),
    }),
  ).toBe("उत्तर सोमवार को शाम 4:31 बजे यहाँ रुका");
});

test("every code's sentence is its own words, never empty", () => {
  for (const code of Object.keys(ERROR_WORDS) as ErrorCode[]) {
    for (const lang of ["en", "hi"] as const) expect(errorWords(lang, code).trim()).not.toBe("");
  }
});

test("each start-up code has its line, distinct, ending in the danda in Hindi, with no banned word", () => {
  const codes: StartupCode[] = [
    "store.newer_version",
    "store.corrupt",
    "store.disk_full",
    "store.io",
  ];
  expect(Object.keys(STARTUP_WORDS).sort()).toEqual([...codes].sort());
  expect(new Set(codes.map((c) => startupWords("en", c))).size).toBe(codes.length);
  for (const [en, hi] of Object.entries(startupWordList)) {
    expect(en).not.toMatch(BANNED_EN);
    expect(hi).not.toMatch(BANNED_HI);
    expect(hi.endsWith("।")).toBe(true);
  }
  expect(startupWords("hi", "store.disk_full")).toContain("डिस्क भर गई");
});

test("the stopped lines fill their time, the part of the day first in Hindi", () => {
  expect(
    brk("en", "The answer stopped at {time}. Your question and the answer so far are kept.", {
      time: clock("en", "4:31 pm"),
    }),
  ).toBe("The answer stopped at 4:31 pm. Your question and the answer so far are kept.");
  expect(brk("hi", "The answer stopped here at {time}", { time: clock("hi", "4:31 pm") })).toBe(
    "उत्तर शाम 4:31 बजे यहाँ रुका",
  );
});

test("item 16's words: no banned word, and each Hindi sentence ends in the danda", () => {
  for (const [en, hi] of Object.entries(breakWords)) {
    expect(en).not.toMatch(BANNED_EN);
    expect(hi).not.toMatch(BANNED_HI);
    if (en.endsWith(".")) expect(hi.endsWith("।")).toBe(true);
  }
});
