// The live thread's new words: each key has Hindi, fills its slots in both languages, ends a Hindi sentence with the
// danda and avoids the banned words; every error code has a sentence, and a code this version doesn't know reads as
// "unknown"'s.
import { readFileSync } from "node:fs";
import { expect, test } from "vitest";
import { clock } from "./words.ts";
import { ERROR_WORDS, errorSentence, errorWords, type ErrorCode } from "./words-errors.ts";
import { frame } from "./words-frame.ts";
import { lim, type LimitKey } from "./words-limits.ts";
import { th, type ThreadKey } from "./words-thread.ts";

const NEW_THREAD: readonly ThreadKey[] = [
  "Failed",
  "Stopped",
  "You stopped the answer. The answer so far is kept.",
  "The answer stopped at its limit, before it finished.",
  "This question was declined, so there is no answer.",
  "The answer stopped. Your question and the answer so far are kept.",
  "Your question",
  "Clauseo's answer",
  "Attached: {name}",
  "Something here can't be shown in the thread.",
  "Clauseo changed 1 paper.",
  "You undid Clauseo's changes to 1 paper.",
];
const LIMITS: readonly LimitKey[] = [
  "Resets {time}",
  "Resets {day} {time}",
  "Limit reached. Reset not given.",
  "Usage limit reached",
  "Try again",
];
const DEVA = /[\u0900-\u097F]/;
const BANNED_HI = /पेंसिल|स्याही|चरण|कदम|(^|\s)मैं(\s|$)|[\u0966-\u096F]/;
const BANNED_EN = /\bagent\b|\btoken\b|\bsession\b/i;

/** Checks a key's English and Hindi: Hindi present, the danda where the English ends a sentence, no banned word. */
function sound(en: string, hi: string) {
  expect(hi, en).toMatch(DEVA);
  expect(hi, en).not.toBe(en);
  expect(en, en).not.toMatch(BANNED_EN);
  expect(hi, en).not.toMatch(BANNED_HI);
  expect(hi, en).not.toMatch(/[\u0900-\u097F]\.$/);
  if (en.endsWith(".")) expect(hi, en).toMatch(/।$/);
}

test("the thread's new keys each have Hindi in the design's voice", () => {
  for (const k of NEW_THREAD) sound(th("en", k), th("hi", k));
});

test("a named attachment fills its slot in both languages", () => {
  expect(th("en", "Attached: {name}", { name: "Notice.pdf" })).toBe("Attached: Notice.pdf");
  expect(th("hi", "Attached: {name}", { name: "Notice.pdf" })).toBe("संलग्न: Notice.pdf");
});

test("the limit's keys each have Hindi, and the reset takes the clock, the part of the day first in Hindi", () => {
  for (const k of LIMITS) sound(lim("en", k), lim("hi", k));
  expect(lim("en", "Resets {time}", { time: clock("en", "6:10 pm") })).toBe("Resets 6:10 pm");
  expect(lim("hi", "Resets {time}", { time: clock("hi", "6:10 pm") })).toBe("शाम 6:10 बजे रीसेट");
  expect(lim("en", "Resets {day} {time}", { day: "Tuesday", time: "9 am" })).toBe(
    "Resets Tuesday 9 am",
  );
  expect(lim("hi", "Resets {day} {time}", { day: "मंगलवार", time: "सुबह 9" })).toBe(
    "मंगलवार सुबह 9 बजे रीसेट",
  );
});

test("the working line says Answering. in both languages", () => {
  expect(frame("en", "Answering.")).toBe("Answering.");
  expect(frame("hi", "Answering.")).toBe("उत्तर दिया जा रहा है।");
});

test("every error code has a sentence with Hindi, and none says agent", () => {
  const codes = Object.keys(ERROR_WORDS) as ErrorCode[];
  expect(codes.length).toBeGreaterThan(30);
  for (const c of codes) sound(errorWords("en", c), errorWords("hi", c));
});

test("a stopped Claude and an interrupted thread read as the thread's own stop, with its one Hindi", () => {
  const stopped = "The answer stopped. Your question and the answer so far are kept.";
  for (const c of ["agent.exited", "thread.interrupted"] as const) {
    expect(errorWords("en", c)).toBe(stopped);
    expect(errorWords("hi", c)).toBe(th("hi", stopped));
  }
});

test("a code this version doesn't know reads as unknown's sentence", () => {
  expect(errorWords("en", "store.melted")).toBe("Something went wrong.");
  expect(errorWords("hi", "store.melted")).toBe(errorSentence("hi", ERROR_WORDS.unknown));
  expect(errorWords("en", "toString")).toBe("Something went wrong.");
});

test.each([["./words-errors.ts"], ["./words-limits.ts"], ["./words-thread.ts"]])(
  "Hindi in %s ends a sentence with the danda and avoids the banned words",
  (file) => {
    const src = readFileSync(new URL(file, import.meta.url), "utf8");
    const hindi = [...src.matchAll(/"([^"]*[\u0900-\u097F][^"]*)"/g)].map((m) => m[1] ?? "");
    expect(hindi.length).toBeGreaterThan(0);
    for (const s of hindi) {
      expect(s, s).not.toMatch(/[\u0900-\u097F]\.$/);
      expect(s, s).not.toMatch(BANNED_HI);
    }
  },
);
