// The web sources' words: the summary's count and its singular, a row's string split at its slot, and the Hindi
// standing in as English until #285.
import { expect, test } from "vitest";
import { partsOf, sourcesSummary, src } from "./words-sources.ts";

test("the summary counts the open list's rows, the singular apart", () => {
  expect(sourcesSummary("en", 5)).toBe("5 sources");
  expect(sourcesSummary("en", 1)).toBe("1 source");
  expect(sourcesSummary("en", 2)).toBe("2 sources");
});

test("each row's words, by kind and agent, with the step list's verbs and quoting", () => {
  expect(partsOf("en", "Searched online for '{query}'.", "query")).toEqual([
    "Searched online for '",
    "'.",
  ]);
  expect(src("en", "No results.")).toBe("No results.");
  expect(src("en", "{name} doesn't report which pages it found.", { name: "ChatGPT" })).toBe(
    "ChatGPT doesn't report which pages it found.",
  );
  expect(partsOf("en", "Read {page} online.", "page")).toEqual(["Read ", " online."]);
  expect(
    src("en", "{name} opened this page on OpenAI's computers, not on this one.", {
      name: "ChatGPT",
    }),
  ).toBe("ChatGPT opened this page on OpenAI's computers, not on this one.");
  expect(partsOf("en", "Opened '{page}' in the browser.", "page")).toEqual([
    "Opened '",
    "' in the browser.",
  ]);
  expect(partsOf("en", "Opened {page} in the browser.", "page")).toEqual([
    "Opened ",
    " in the browser.",
  ]);
});

test("every string is hi: later, and the Hindi interface shows its English", () => {
  expect(Object.values(src.hi).every((hi) => hi === null)).toBe(true);
  expect(sourcesSummary("hi", 2)).toBe("2 sources");
  expect(partsOf("hi", "Read {page} online.", "page")).toEqual(["Read ", " online."]);
});
