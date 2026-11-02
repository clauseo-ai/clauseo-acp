// conversation-title.ts: a new conversation's title from its first message, the host's rules for a title, and the
// host's limit on a title's length, read from crates/protocol/src/title.rs.
import { readFileSync } from "node:fs";
import { expect, test } from "vitest";
import { isTitle, MAX_TITLE_CHARS, titleOf } from "./conversation-title.ts";

const points = (text: string) => Array.from(text).length;

test("a message of 48 code points or fewer is its own title, whole", () => {
  const at48 = "word ".repeat(9) + "abc";
  expect(points(at48)).toBe(48);
  expect(titleOf(at48)).toBe(at48);
});

test("a message of 49 code points gives its first 6 words", () => {
  const at49 = "word ".repeat(9) + "abcd";
  expect(points(at49)).toBe(49);
  expect(titleOf(at49)).toBe("word word word word word word");
});

test("whitespace is collapsed and trimmed, tabs and newlines too", () => {
  expect(titleOf("  Draft\n\n the   reply  ")).toBe("Draft the reply");
  expect(titleOf("Draft\tthe\r\nreply")).toBe("Draft the reply");
});

test("a longer message gives its first 6 words, joined by one space", () => {
  const long =
    "Please prepare the bail application for the accused in Delhi court tomorrow morning";
  expect(points(long)).toBeGreaterThan(48);
  expect(titleOf(long)).toBe("Please prepare the bail application for");
});

test("a long message of fewer than 6 words gives those words, even over 48 code points", () => {
  const a = "x".repeat(30);
  const b = "y".repeat(30);
  expect(titleOf(`${a}   ${b}`)).toBe(`${a} ${b}`);
  // one word of 60 code points is the title, as it is still within the host's limit
  expect(titleOf("z".repeat(60))).toBe("z".repeat(60));
});

test("a Devanagari question of 48 code points or fewer stays whole, combining vowel signs and all", () => {
  const question = "मुझे किसी वकील से बात करनी है";
  // 29 code points, the vowel signs ि and ी among them: each is its own code point
  expect(points(question)).toBe(29);
  expect(titleOf(question)).toBe(question);
});

test("a long Devanagari message gives its first 6 words", () => {
  const long = "मुझे किसी वकील से बात करनी है और मेरे केस की तारीख भी बताइए";
  expect(points(long)).toBeGreaterThan(48);
  expect(titleOf(long)).toBe("मुझे किसी वकील से बात करनी");
});

test("the length is in code points, not UTF-16 units: 47 letters and an emoji are 48, whole", () => {
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
  // six 40-letter words: 245 code points, over the 200 limit
  expect(titleOf(Array(6).fill("a".repeat(40)).join(" "))).toBeNull();
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
