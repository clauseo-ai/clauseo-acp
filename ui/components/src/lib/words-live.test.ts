// The live thread's new words: each key has Hindi, fills its slots in both languages, ends a Hindi sentence with the
// danda and avoids the banned words; every error code has a sentence, and a code this version doesn't know reads as
// "unknown"'s.
import { readFileSync } from "node:fs";
import { expect, test } from "vitest";
import { clock } from "./words.ts";
import {
  ERROR_WORDS,
  errorSentence,
  errorWords,
  plannedKey,
  PLANNED_CODES,
  type ErrorCode,
} from "./words-errors.ts";
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
  "Your question, {time}",
  "Clauseo's answer, {time}",
  "Attached: {name}",
  "Something here can't be shown in the thread.",
  "Clauseo changed 1 paper.",
  "You undid Clauseo's changes to 1 paper.",
];
const LIMITS: readonly LimitKey[] = [
  "Resets {time}",
  "Resets {day} {time}",
  "Limit reached. Reset not given.",
  "Reset not given.",
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
  // and the sentences a thread's agent picks (the agent.* codes'): ChatGPT's, a named agent's with a sample name, so
  // our own words around the slot are checked (a settings id itself is the lawyer's label, shown as written), and the
  // nameless one
  for (const c of PLANNED_CODES)
    for (const agent of [CODEX, named("Gemini"), named(null)])
      sound(errorWords("en", c, agent), errorWords("hi", c, agent));
});

test("a stopped Claude and an interrupted thread read as the thread's own stop, with its one Hindi", () => {
  const stopped = "The answer stopped. Your question and the answer so far are kept.";
  for (const c of ["agent.exited", "thread.interrupted"] as const) {
    expect(errorWords("en", c)).toBe(stopped);
    expect(errorWords("hi", c)).toBe(th("hi", stopped));
  }
});

/** A thread's agent as errorWords takes it. */
const CODEX = { kind: "codex", name: "codex-acp" } as const;
const CLAUDE = { kind: "claude", name: "claude-acp" } as const;
const named = (name: string | null) => ({ kind: undefined, name });

// [code, ChatGPT's English and Hindi, a named agent's (gemini) English and Hindi, the nameless English and Hindi]
const PLANNED_LINES = [
  [
    "agent.launch_failed",
    "ChatGPT didn't start. Your question is kept.",
    "ChatGPT शुरू नहीं हुआ। आपका प्रश्न रखा गया है।",
    "gemini didn't start. Your question is kept.",
    "gemini शुरू नहीं हुआ। आपका प्रश्न रखा गया है।",
    "The AI didn't start. Your question is kept.",
    "AI शुरू नहीं हुआ। आपका प्रश्न रखा गया है।",
  ],
  [
    "agent.protocol_error",
    "ChatGPT sent something Clauseo can't read.",
    "ChatGPT ने कुछ ऐसा भेजा जिसे Clauseo पढ़ नहीं सकता।",
    "gemini sent something Clauseo can't read.",
    "gemini ने कुछ ऐसा भेजा जिसे Clauseo पढ़ नहीं सकता।",
    "The AI sent something Clauseo can't read.",
    "AI ने कुछ ऐसा भेजा जिसे Clauseo पढ़ नहीं सकता।",
  ],
  [
    "agent.auth_required",
    "Sign in to ChatGPT in Codex first. Your question is kept.",
    "पहले Codex में ChatGPT से साइन इन करें। आपका प्रश्न रखा गया है।",
    "Sign in to gemini first. Your question is kept.",
    "पहले gemini में साइन इन करें। आपका प्रश्न रखा गया है।",
    "Sign in to the AI first. Your question is kept.",
    "पहले AI में साइन इन करें। आपका प्रश्न रखा गया है।",
  ],
  [
    "agent.unsupported_version",
    "This version of Codex doesn't work with Clauseo.",
    "Codex का यह संस्करण Clauseo के साथ काम नहीं करता।",
    "This version of gemini doesn't work with Clauseo.",
    "gemini का यह संस्करण Clauseo के साथ काम नहीं करता।",
    "This version of the AI doesn't work with Clauseo.",
    "AI का यह संस्करण Clauseo के साथ काम नहीं करता।",
  ],
] as const;

test.each(PLANNED_LINES)(
  "%s names the thread's agent: ChatGPT's line, a named agent's, the nameless one, Claude's otherwise",
  (code, en, hi, namedEn, namedHi, namelessEn, namelessHi) => {
    expect(errorWords("en", code, CODEX)).toBe(en);
    expect(errorWords("hi", code, CODEX)).toBe(hi);
    expect(errorWords("en", code, named("gemini"))).toBe(namedEn);
    expect(errorWords("hi", code, named("gemini"))).toBe(namedHi);
    for (const none of [null, "", "  "]) {
      expect(errorWords("en", code, named(none))).toBe(namelessEn);
      expect(errorWords("hi", code, named(none))).toBe(namelessHi);
    }
    // the kind picks the line, never the name: an agent of no kind whose id reads like a plan is named by it
    expect(errorWords("en", code, named("ChatGPT"))).toBe(namedEn.replace("gemini", "ChatGPT"));
    expect(errorWords("en", code, named("Claude"))).toBe(namedEn.replace("gemini", "Claude"));
    // a Claude thread, or a caller that doesn't know the thread, keeps the approved line
    for (const agent of [CLAUDE, undefined]) {
      expect(errorWords("en", code, agent)).toBe(ERROR_WORDS[code]);
      expect(errorWords("hi", code, agent)).toBe(errorWords("hi", code));
      expect(plannedKey(code, agent).key).toBe(ERROR_WORDS[code]);
    }
    expect(errorWords("en", code)).toContain("Claude");
  },
);

test("an agent's name fills no other code", () => {
  expect(errorWords("en", "store.io", CODEX)).toBe(errorWords("en", "store.io"));
  expect(errorWords("en", "store.io", named("gemini"))).toBe(errorWords("en", "store.io"));
  expect(errorWords("en", "thread.interrupted", named("gemini"))).toBe(
    errorWords("en", "thread.interrupted"),
  );
  // a settings id is shown as the lawyer wrote it, even one the words' own rules would ban
  expect(errorWords("en", "agent.launch_failed", named("my-agent"))).toBe(
    "my-agent didn't start. Your question is kept.",
  );
});

test("only the agent.* codes that name Claude have a sentence per plan", () => {
  const naming = (Object.keys(ERROR_WORDS) as ErrorCode[]).filter((c) =>
    ERROR_WORDS[c].includes("Claude"),
  );
  expect([...PLANNED_CODES].sort()).toEqual(naming.sort());
});

test("a code this version doesn't know reads as unknown's sentence, which is internal's", () => {
  expect(ERROR_WORDS.unknown).toBe(ERROR_WORDS.internal);
  expect(errorWords("en", "store.melted")).toBe("Something went wrong inside Clauseo.");
  expect(errorWords("hi", "store.melted")).toBe(errorSentence("hi", ERROR_WORDS.unknown));
  expect(errorWords("en", "toString")).toBe("Something went wrong inside Clauseo.");
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
