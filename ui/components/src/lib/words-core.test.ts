// hi: later. A new string may ship with null Hindi: every key still needs its entry, and the Hindi interface shows the
// English, its slots filled as in English.
import { expect, test } from "vitest";
import { wordsOf, type Hindi } from "./words-core.ts";

const WORDS = {
  "Decided {date}": "निर्णय दिनांक {date}",
  "Hearing on {date} at {time}": null,
} as const;
type K = keyof typeof WORDS;
const say = wordsOf<K>(WORDS);

test("a words list takes null Hindi per entry, and a missing key or an undefined one is still a type error", () => {
  const later: Hindi<"Open" | "Close"> = { Open: "खोलें", Close: null };
  // @ts-expect-error: Close has no entry
  const missing: Hindi<"Open" | "Close"> = { Open: "खोलें" };
  // @ts-expect-error: undefined is not hi: later; null is
  const blank: Hindi<"Open" | "Close"> = { Open: "खोलें", Close: undefined };
  // @ts-expect-error: wordsOf wants every key of its list
  wordsOf<"Open" | "Close">({ Open: "खोलें" });
  // @ts-expect-error: a key the list doesn't hold is no key
  say("en", "Hearing at {time}");
  expect([later, missing, blank].map((w) => Object.keys(w).length)).toEqual([2, 1, 2]);
});

test("in Hindi, a null entry says its English with its slots filled; Hindi entries are unchanged", () => {
  const slots = { date: "4 November 2026", time: "10:30 am" };
  expect(say("hi", "Hearing on {date} at {time}", slots)).toBe(
    "Hearing on 4 November 2026 at 10:30 am",
  );
  expect(say("en", "Hearing on {date} at {time}", slots)).toBe(
    "Hearing on 4 November 2026 at 10:30 am",
  );
  // a slot the caller didn't give stays as written, as in English
  expect(say("hi", "Hearing on {date} at {time}", { date: "4 November 2026" })).toBe(
    "Hearing on 4 November 2026 at {time}",
  );
  expect(say("hi", "Decided {date}", slots)).toBe("निर्णय दिनांक 4 November 2026");
});

test("the lookup carries its list, so the copy review can tell hi: later from Hindi", () => {
  expect(say.hi["Hearing on {date} at {time}"]).toBeNull();
  expect(say.hi["Decided {date}"]).toBe("निर्णय दिनांक {date}");
});
