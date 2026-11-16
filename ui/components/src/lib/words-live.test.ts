// The live thread's new words: each key has Hindi, fills its slots in both languages, ends a Hindi sentence with the
// danda and avoids the banned words; every error code has a sentence, and a code this version doesn't know reads as
// "unknown"'s.
import { readFileSync } from "node:fs";
import { expect, test } from "vitest";
import { clock } from "./words.ts";
import {
  AGENTS_FOLDER_MAC,
  ERROR_WORDS,
  errorSentence,
  GIT_FOLDER_MAC,
  GIT_FOLDER_PLANNED,
  errorWords,
  plannedKey,
  PLANNED_CODES,
  type WordedCode,
} from "./words-errors.ts";
import { errorHindi } from "./words-errors.hi.ts";
import { fr, type FolderKey } from "./words-folder.ts";
import { frc, type FolderCardKey } from "./words-folder-card.ts";
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
  "Send",
  "Don't answer",
  "Other",
  "Required",
  "Answer this question.",
  "This answer doesn't fit the question.",
  "Choose an answer to send, or 'Don't answer' to go on without one.",
  "This question can't be shown here. Choose 'Don't answer', or Stop the answer.",
  "Use {n} characters or fewer.",
  "Use 1 character or fewer.",
  "Use at least {n} characters.",
  "Choose up to {n}.",
  "Choose at least {n}.",
  "Use a number from {min} to {max}.",
  "Use {min} or more.",
  "Use {max} or less.",
  "Use a whole number.",
  "Latest",
  "Go to the latest message",
];
const LIMITS: readonly LimitKey[] = [
  "Resets {time}",
  "Resets {day} {time}",
  "Limit reached",
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

test("every error code has a sentence with Hindi or Hindi later, and none says agent", () => {
  const codes = Object.keys(ERROR_WORDS) as WordedCode[];
  expect(codes.length).toBeGreaterThan(30);
  // the stopped sentence is the thread's, so it is not in errorHindi and reads as undefined here
  const hindi: Readonly<Record<string, string | null>> = errorHindi;
  for (const c of codes) {
    const en = errorWords("en", c);
    // hi: later (Rohan, 2026-11-03): new strings ship in English; their English is still checked
    if (hindi[ERROR_WORDS[c]] === null) {
      expect(en, en).not.toMatch(BANNED_EN);
      continue;
    }
    sound(en, errorWords("hi", c));
  }
  // and the sentences a thread's agent picks (the agent.* codes'): ChatGPT's, a named agent's with a sample name, so
  // our own words around the slot are checked (a settings id itself is the lawyer's label, shown as written), and the
  // nameless one
  for (const c of PLANNED_CODES)
    for (const agent of [CODEX, named("Gemini"), named(null)]) {
      const en = errorWords("en", c, agent);
      // hi: later (Rohan, 2026-11-03): a sentence whose Hindi is null ships in English, as above
      if (hindi[plannedKey(c, agent).key] === null) expect(en, en).not.toMatch(BANNED_EN);
      else sound(en, errorWords("hi", c, agent));
    }
});

test("folder rights' words have their Hindi, and none says agent, token or session", () => {
  for (const k of Object.keys(fr.hi) as FolderKey[]) {
    expect(fr.hi[k], k).not.toBeNull();
    expect(fr("en", k), k).not.toMatch(BANNED_EN);
  }
  for (const k of Object.keys(frc.hi) as FolderCardKey[]) {
    expect(frc.hi[k], k).not.toBeNull();
    expect(frc("en", k), k).not.toMatch(BANNED_EN);
  }
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
    "Sign in with ChatGPT in Codex first. Your question is kept.",
    "पहले Codex में ChatGPT से साइन इन करें। आपका प्रश्न रखा गया है।",
    "Sign in with gemini first. Your question is kept.",
    "पहले gemini से साइन इन करें। आपका प्रश्न रखा गया है।",
    "Sign in first. Your question is kept.",
    "पहले साइन इन करें। आपका प्रश्न रखा गया है।",
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
  [
    "agent.guard_missing",
    "Clauseo can't start ChatGPT safely here. Reinstall Clauseo.",
    "Clauseo यहाँ ChatGPT को सुरक्षित रूप से शुरू नहीं कर सकता। Clauseo फिर से इंस्टॉल करें।",
    "Clauseo can't start gemini safely here. Reinstall Clauseo.",
    "Clauseo यहाँ gemini को सुरक्षित रूप से शुरू नहीं कर सकता। Clauseo फिर से इंस्टॉल करें।",
    "Clauseo can't start the AI safely here. Reinstall Clauseo.",
    "Clauseo यहाँ AI को सुरक्षित रूप से शुरू नहीं कर सकता। Clauseo फिर से इंस्टॉल करें।",
  ],
  [
    "agent.service_error",
    "ChatGPT had a problem on its side. Try again.",
    "ChatGPT की ओर से कोई समस्या आई। फिर से कोशिश करें।",
    "gemini had a problem on its side. Try again.",
    "gemini की ओर से कोई समस्या आई। फिर से कोशिश करें।",
    "The AI had a problem on its side. Try again.",
    "AI की ओर से कोई समस्या आई। फिर से कोशिश करें।",
  ],
  [
    "agent.quiet",
    "ChatGPT stopped answering. Try again.",
    "ChatGPT ने उत्तर देना बंद कर दिया। फिर से प्रयास करें।",
    "gemini stopped answering. Try again.",
    "gemini ने उत्तर देना बंद कर दिया। फिर से प्रयास करें।",
    "The AI stopped answering. Try again.",
    "AI ने उत्तर देना बंद कर दिया। फिर से प्रयास करें।",
  ],
  [
    "matter.folder_backslash",
    "ChatGPT can't work in this matter's folder because its name has a backslash (\\). Rename the folder in Finder, then Try again.",
    "ChatGPT इस मामले के फ़ोल्डर में काम नहीं कर सकता, क्योंकि उसके नाम में बैकस्लैश (\\) है। Finder में फ़ोल्डर का नाम बदलें, उसके बाद फिर से प्रयास करें।",
    "gemini can't work in this matter's folder because its name has a backslash (\\). Rename the folder in Finder, then Try again.",
    "gemini इस मामले के फ़ोल्डर में काम नहीं कर सकता, क्योंकि उसके नाम में बैकस्लैश (\\) है। Finder में फ़ोल्डर का नाम बदलें, उसके बाद फिर से प्रयास करें।",
    "The AI can't work in this matter's folder because its name has a backslash (\\). Rename the folder in Finder, then Try again.",
    "AI इस मामले के फ़ोल्डर में काम नहीं कर सकता, क्योंकि उसके नाम में बैकस्लैश (\\) है। Finder में फ़ोल्डर का नाम बदलें, उसके बाद फिर से प्रयास करें।",
  ],
  [
    "agent.folder_inaccessible",
    "ChatGPT couldn't open the matter's folder. Check that it's still there and hasn't been renamed, then Try again.",
    "ChatGPT मामले का फ़ोल्डर नहीं खोल सका। जाँचें कि वह अब भी वहीं है और उसका नाम नहीं बदला गया है, उसके बाद फिर से प्रयास करें।",
    "gemini couldn't open the matter's folder. Check that it's still there and hasn't been renamed, then Try again.",
    "gemini मामले का फ़ोल्डर नहीं खोल सका। जाँचें कि वह अब भी वहीं है और उसका नाम नहीं बदला गया है, उसके बाद फिर से प्रयास करें।",
    "The AI couldn't open the matter's folder. Check that it's still there and hasn't been renamed, then Try again.",
    "AI मामले का फ़ोल्डर नहीं खोल सका। जाँचें कि वह अब भी वहीं है और उसका नाम नहीं बदला गया है, उसके बाद फिर से प्रयास करें।",
  ],
  [
    "agent.unreachable",
    "Clauseo couldn't reach ChatGPT. The answer so far is kept. Try again. If this keeps happening, ask your firm's IT.",
    "Clauseo, ChatGPT तक नहीं पहुँच सका। अब तक का उत्तर रखा गया है। फिर से कोशिश करें। अगर ऐसा बार-बार हो, तो अपनी फ़र्म की IT टीम से पूछें।",
    "Clauseo couldn't reach gemini. The answer so far is kept. Try again. If this keeps happening, ask your firm's IT.",
    "Clauseo, gemini तक नहीं पहुँच सका। अब तक का उत्तर रखा गया है। फिर से कोशिश करें। अगर ऐसा बार-बार हो, तो अपनी फ़र्म की IT टीम से पूछें।",
    "Clauseo couldn't reach the AI. The answer so far is kept. Try again. If this keeps happening, ask your firm's IT.",
    "Clauseo, AI तक नहीं पहुँच सका। अब तक का उत्तर रखा गया है। फिर से कोशिश करें। अगर ऐसा बार-बार हो, तो अपनी फ़र्म की IT टीम से पूछें।",
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
  const naming = (Object.keys(ERROR_WORDS) as WordedCode[]).filter((c) =>
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

test.each([["./words-errors.hi.ts"], ["./words-limits.hi.ts"], ["./words-thread.hi.ts"]])(
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

// matter.folder_backslash by where the backslash is and the computer: the matter folder's own name, or the nearest folder
// above it by name (isolated), and Finder on the Mac, "your file manager" on Linux; each agent's line, in both languages
const FOLDER = "\u2068Old\\Work\u2069";
test.each([
  [
    "mac",
    null,
    "can't work in this matter's folder because its name has a backslash (\\). Rename the folder in Finder, then Try again.",
    "इस मामले के फ़ोल्डर में काम नहीं कर सकता, क्योंकि उसके नाम में बैकस्लैश (\\) है। Finder में फ़ोल्डर का नाम बदलें, उसके बाद फिर से प्रयास करें।",
  ],
  [
    "linux",
    null,
    "can't work in this matter's folder because its name has a backslash (\\). Rename the folder in your file manager, then Try again.",
    "इस मामले के फ़ोल्डर में काम नहीं कर सकता, क्योंकि उसके नाम में बैकस्लैश (\\) है। अपने फ़ाइल मैनेजर में फ़ोल्डर का नाम बदलें, उसके बाद फिर से प्रयास करें।",
  ],
  [
    "mac",
    "Old\\Work",
    `can't work in this matter's folder because the folder "${FOLDER}" in its path has a backslash (\\). Rename that folder in Finder, then Try again.`,
    `इस मामले के फ़ोल्डर में काम नहीं कर सकता, क्योंकि उसके पाथ में मौजूद फ़ोल्डर "${FOLDER}" के नाम में बैकस्लैश (\\) है। Finder में उस फ़ोल्डर का नाम बदलें, उसके बाद फिर से प्रयास करें।`,
  ],
  [
    "linux",
    "Old\\Work",
    `can't work in this matter's folder because the folder "${FOLDER}" in its path has a backslash (\\). Rename that folder in your file manager, then Try again.`,
    `इस मामले के फ़ोल्डर में काम नहीं कर सकता, क्योंकि उसके पाथ में मौजूद फ़ोल्डर "${FOLDER}" के नाम में बैकस्लैश (\\) है। अपने फ़ाइल मैनेजर में उस फ़ोल्डर का नाम बदलें, उसके बाद फिर से प्रयास करें।`,
  ],
] as const)(
  "matter.folder_backslash on %s, the folder above with it %j: each agent's line",
  (platform, parent, en, hi) => {
    const place = { platform, parent };
    const code = "matter.folder_backslash";
    for (const [agent, enName, hiName] of [
      [CLAUDE, "Claude", "Claude"],
      [undefined, "Claude", "Claude"],
      [CODEX, "ChatGPT", "ChatGPT"],
      [named("gemini"), "gemini", "gemini"],
      [named(null), "The AI", "AI"],
    ] as const) {
      expect(errorWords("en", code, agent, place)).toBe(`${enName} ${en}`);
      expect(errorWords("hi", code, agent, place)).toBe(`${hiName} ${hi}`);
      sound(errorWords("en", code, agent, place), errorWords("hi", code, agent, place));
    }
    // the place reaches no other code's line
    expect(errorWords("en", "agent.folder_inaccessible", CLAUDE, place)).toBe(
      ERROR_WORDS["agent.folder_inaccessible"],
    );
  },
);

test("matter.folder_backslash with no place known: the Mac's line for the folder's own name, as before", () => {
  expect(errorWords("en", "matter.folder_backslash")).toBe(
    errorWords("en", "matter.folder_backslash", undefined, { platform: "mac", parent: null }),
  );
});

// matter.git_folder (U3-err-matter.git_folder and its variants): Claude's line, ChatGPT's, a named agent's and the
// nameless one's, each with its Mac line (its .mac id) on the Mac only, and English on the Mac too (Hindi later)
test("matter.git_folder: each agent's line; its Mac line on the Mac only, the base line elsewhere", () => {
  const code = "matter.git_folder";
  const tail =
    " Finder hides folders whose names start with a dot; press Command-Shift-period in Finder to show them.";
  const lines = [
    [undefined, ERROR_WORDS[code], GIT_FOLDER_MAC.claude],
    [CLAUDE, ERROR_WORDS[code], GIT_FOLDER_MAC.claude],
    [CODEX, GIT_FOLDER_PLANNED.chatgpt, GIT_FOLDER_MAC.chatgpt],
    [
      named("gemini"),
      GIT_FOLDER_PLANNED.named.replaceAll("{name}", "gemini"),
      GIT_FOLDER_MAC.named.replaceAll("{name}", "gemini"),
    ],
    [named(null), GIT_FOLDER_PLANNED.nameless, GIT_FOLDER_MAC.nameless],
  ] as const;
  for (const [agent, base, mac] of lines) {
    expect(errorWords("en", code, agent)).toBe(base);
    expect(errorWords("en", code, agent, { platform: "windows" })).toBe(base);
    expect(errorWords("en", code, agent, { platform: "linux" })).toBe(base);
    expect(errorWords("en", code, agent, { platform: "mac" })).toBe(mac);
    expect(errorWords("hi", code, agent, { platform: "mac" })).toBe(mac);
  }
  // the Mac lines are the base lines plus the Finder sentence
  for (const variant of Object.values(GIT_FOLDER_MAC)) expect(variant.endsWith(tail)).toBe(true);
  expect(GIT_FOLDER_MAC.chatgpt).toBe(`${GIT_FOLDER_PLANNED.chatgpt}${tail}`);
  // ChatGPT's line names ChatGPT, not Claude, and no name slot is left unfilled
  expect(errorWords("en", code, CODEX)).toContain("Clauseo doesn't start ChatGPT in a folder");
  expect(errorWords("en", code, CODEX)).not.toContain("Claude");
  expect(errorWords("en", code, named("gemini"))).not.toContain("{name}");
  // the Mac's pick reaches no other code's line
  expect(errorWords("en", "agent.folder_inaccessible", CLAUDE, { platform: "mac" })).toBe(
    ERROR_WORDS["agent.folder_inaccessible"],
  );
});

// matter.agents_folder on the Mac (U3-err-matter.agents_folder.mac): the base line, then how to show the folders Finder
// hides. Picked by the computer's platform alone, on the Mac only; Hindi later (null), so the English shows in both.
test("matter.agents_folder on the Mac is its .mac line; elsewhere, and with no platform, the base line", () => {
  const code = "matter.agents_folder";
  const base = ERROR_WORDS[code];
  expect(AGENTS_FOLDER_MAC).toBe(
    `${base} Finder hides folders whose names start with a dot; press Command-Shift-period in Finder to show them.`,
  );
  expect(errorHindi[AGENTS_FOLDER_MAC]).toBeNull();
  for (const agent of [CODEX, CLAUDE, undefined]) {
    expect(errorWords("en", code, agent, { platform: "mac" })).toBe(AGENTS_FOLDER_MAC);
    expect(errorWords("hi", code, agent, { platform: "mac" })).toBe(AGENTS_FOLDER_MAC);
    expect(errorWords("en", code, agent, { platform: "windows" })).toBe(base);
    expect(errorWords("en", code, agent, { platform: "linux" })).toBe(base);
    expect(errorWords("en", code, agent)).toBe(base);
  }
  // the Mac's line is the one code's only: another code's line stays as it was on the Mac
  expect(errorWords("en", "agent.folder_inaccessible", CLAUDE, { platform: "mac" })).toBe(
    ERROR_WORDS["agent.folder_inaccessible"],
  );
});
