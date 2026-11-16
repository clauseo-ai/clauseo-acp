// conversation-title.ts: a new conversation's title from its first message, the host's rules for a title, and the
// host's limit on a title's length, read from crates/protocol/src/title.rs.
import { readFileSync } from "node:fs";
import { expect, test } from "vitest";
import { isTitle, MAX_TITLE_CHARS, titleOf } from "./conversation-title.ts";

const points = (text: string) => Array.from(text).length;
const graphemes = (text: string) =>
  Array.from(new Intl.Segmenter(undefined, { granularity: "grapheme" }).segment(text)).length;

// D-070: [the first message, its title]. Whole up to 48 graphemes; longer, cut with "…" at the first clause's end
// (16 graphemes or more), else the last space, never mid-word; "…" only when cut.
const TITLES: [message: string, title: string][] = [
  // whole
  ["Draft the reply", "Draft the reply"],
  ["word ".repeat(9) + "abc", "word ".repeat(9) + "abc"],
  ["मुझे किसी वकील से बात करनी है", "मुझे किसी वकील से बात करनी है"],
  ["x".repeat(47) + "📄", "x".repeat(47) + "📄"],
  // one grapheme over: the last space, with its "…"
  ["word ".repeat(9) + "abcd", "word word word word word word word word word…"],
  // the stage C case: the boilerplate dropped, then the first clause would be too long, so the last space
  [
    "Without using web search, write a short note on the notice period in clause 9 of the lease deed",
    "Write a short note on the notice period in…",
  ],
  ["Without web search: what is a caveat?", "What is a caveat?"],
  // the first clause's end, when it comes late enough
  [
    "Please read the lease deed, and tell me whether clause nine lets the bank end it early",
    "Please read the lease deed…",
  ],
  [
    "What does clause nine say? Then draft a reply to the bank's notice of the twelfth",
    "What does clause nine say…",
  ],
  // a clause too short to stand alone: on to the last space
  [
    "Hi, can you prepare the bail application for the accused in Delhi court tomorrow morning",
    "Hi, can you prepare the bail application for…",
  ],
  [
    "Please prepare the bail application for the accused in Delhi court tomorrow morning",
    "Please prepare the bail application for the…",
  ],
  // Hindi: cut at a danda's end, else at a space, a vowel sign never parted from its letter
  [
    "किरायानामे की धारा नौ पढ़िए। फिर बैंक के नोटिस का जवाब तैयार कीजिए और तारीख भी बताइए",
    "किरायानामे की धारा नौ पढ़िए…",
  ],
  [
    "मुझे किसी वकील से बात करनी है और मेरे केस की तारीख भी बताइए कृपया जल्दी",
    "मुझे किसी वकील से बात करनी है और मेरे केस की तारीख भी बताइए कृपया…",
  ],
  // only a word longer than the whole title is cut inside it
  ["z".repeat(60), "z".repeat(47) + "…"],
  ["x".repeat(30) + "   " + "y".repeat(30), "x".repeat(30) + "…"],
];

test.each(TITLES)("%j gives %j", (message, title) => {
  expect(titleOf(message)).toBe(title);
  expect(graphemes(title)).toBeLessThanOrEqual(48);
  expect(title.endsWith("…")).toBe(graphemes(message.trim().replace(/\s+/g, " ")) > 48);
});

test("a cut never parts a Hindi letter from its vowel sign or virama", () => {
  // one word of 80 graphemes, अनुच्छेद (अ नु च्छे द) twenty times: cut inside it, at a grapheme's edge
  const long = "अनुच्छेद".repeat(20);
  const title = titleOf(long) ?? "";
  const segments = (text: string) =>
    Array.from(
      new Intl.Segmenter(undefined, { granularity: "grapheme" }).segment(text),
      (g) => g.segment,
    );
  const kept = segments(title.slice(0, -1));
  expect(title.endsWith("…")).toBe(true);
  expect(kept).toEqual(segments(long).slice(0, 47));
  // never ending on a bare virama, the conjunct च्छे whole
  expect(title.slice(0, -1).endsWith("\u094d")).toBe(false);
});

test("whitespace is collapsed and trimmed, tabs and newlines too", () => {
  expect(titleOf("  Draft\n\n the   reply  ")).toBe("Draft the reply");
  expect(titleOf("Draft\tthe\r\nreply")).toBe("Draft the reply");
});

test("the boilerplate alone is the title as it is, never nothing", () => {
  expect(titleOf("Without using web search,")).toBe("Without using web search,");
});

test("the length is in graphemes: 47 letters and an emoji are 48, whole, though 49 UTF-16 units", () => {
  const title = "x".repeat(47) + "📄";
  expect(points(title)).toBe(48);
  expect(title.length).toBe(49);
  expect(titleOf(title)).toBe(title);
});

test("a message the host would refuse gives null", () => {
  // nothing that shows
  expect(titleOf("\u200b\u200b")).toBeNull();
  // a control character
  expect(titleOf("Draft\u0007 reply")).toBeNull();
  // a bidi override
  expect(titleOf("\u202eabc")).toBeNull();
  // nothing at all, or whitespace only
  expect(titleOf("")).toBeNull();
  expect(titleOf(" \n\t ")).toBeNull();
});

test("isTitle takes a visible title and refuses a lone combining mark", () => {
  expect(isTitle("Bail application")).toBe(true);
  // a combining mark shows only on a character before it
  expect(isTitle("\u0301")).toBe(false);
  // e followed by a combining acute is a visible character
  expect(isTitle("e\u0301")).toBe(true);
});

test("isTitle takes up to MAX_TITLE_CHARS code points and no more", () => {
  expect(isTitle("a".repeat(MAX_TITLE_CHARS))).toBe(true);
  expect(isTitle("a".repeat(MAX_TITLE_CHARS + 1))).toBe(false);
});

test("MAX_TITLE_CHARS is the host's MAX_TITLE_CHARS in crates/protocol/src/title.rs", () => {
  const rust = readFileSync(
    new URL("../../../../crates/protocol/src/title.rs", import.meta.url),
    "utf8",
  );
  const match = rust.match(/pub const MAX_TITLE_CHARS: usize = (\d+);/);
  expect(match).not.toBeNull();
  expect(Number(match?.[1])).toBe(MAX_TITLE_CHARS);
});
