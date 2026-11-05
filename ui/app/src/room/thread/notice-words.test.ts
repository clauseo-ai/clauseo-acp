// noticeLine: the app's line for each notice kind it knows, and null for everything else, so the row shows the notice's
// text. Read by shape: an absent kind, an unknown string, and an object this version doesn't know give null.
import { t, type Key } from "@clauseo/components/lib/words.ts";
import { th, type ThreadKey } from "@clauseo/components/lib/words-thread.ts";
import { expect, test } from "vitest";
import type { NoticeKind } from "../../transport/protocol.ts";
import { noticeLine, type NoticeWords } from "./notice-words.ts";

const words = (lang: "en" | "hi"): NoticeWords => ({
  lang,
  th: (key: ThreadKey, slots) => th(lang, key, slots),
  t: (key: Key, slots) => t(lang, key, slots),
});
const en = words("en");

const CONTEXT =
  "Clauseo doesn't remember the earlier questions and answers in this conversation. Include what it needs from them in your next question.";

test("a known kind gives the app's line, in the page's words", () => {
  expect(noticeLine("context_not_restored", en)).toBe(CONTEXT);
});

test("no kind (an agent's notice) gives null, so the row shows its own text", () => {
  expect(noticeLine(undefined, en)).toBeNull();
});

test("a kind this version does not know gives null, so the row shows its text", () => {
  expect(noticeLine("something_newer" as NoticeKind, en)).toBeNull();
});

test("an object kind this version does not know gives null, so the row shows its text", () => {
  expect(noticeLine({ other: {} } as unknown as NoticeKind, en)).toBeNull();
  expect(noticeLine(null as unknown as NoticeKind, en)).toBeNull();
});

test("the Hindi line is the English until its Hindi comes (hi: later)", () => {
  expect(noticeLine("context_not_restored", words("hi"))).toBe(CONTEXT);
});
