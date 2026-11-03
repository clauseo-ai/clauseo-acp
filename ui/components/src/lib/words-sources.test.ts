// The web sources' words: the summary's plurals and either sentence alone, a row's string split at its slot, and the
// Hindi standing in as English until #285.
import { expect, test } from "vitest";
import { partsOf, sourcesSummary, src } from "./words-sources.ts";

test("the summary says both counts, the singular apart, and either alone", () => {
  expect(sourcesSummary("en", 2, 3)).toBe("2 web searches. 3 pages read.");
  expect(sourcesSummary("en", 1, 1)).toBe("1 web search. 1 page read.");
  expect(sourcesSummary("en", 4, 0)).toBe("4 web searches.");
  expect(sourcesSummary("en", 1, 0)).toBe("1 web search.");
  expect(sourcesSummary("en", 0, 5)).toBe("5 pages read.");
  expect(sourcesSummary("en", 0, 1)).toBe("1 page read.");
  expect(sourcesSummary("en", 0, 0)).toBe("");
});

test("each row's words, by kind and agent", () => {
  expect(partsOf("en", "Searched the web for {query}.", "query")).toEqual([
    "Searched the web for ",
    ".",
  ]);
  expect(src("en", "No results.")).toBe("No results.");
  expect(src("en", "{name} doesn't report which pages it found.", { name: "ChatGPT" })).toBe(
    "ChatGPT doesn't report which pages it found.",
  );
  expect(partsOf("en", "Read {page}.", "page")).toEqual(["Read ", "."]);
  expect(
    src("en", "{name} opened this page on OpenAI's computers, not on this one.", {
      name: "ChatGPT",
    }),
  ).toBe("ChatGPT opened this page on OpenAI's computers, not on this one.");
  expect(partsOf("en", "Opened {page} in the browser.", "page")).toEqual([
    "Opened ",
    " in the browser.",
  ]);
});

test("every string is hi: later, and the Hindi interface shows its English", () => {
  expect(Object.values(src.hi).every((hi) => hi === null)).toBe(true);
  expect(sourcesSummary("hi", 2, 1)).toBe("2 web searches. 1 page read.");
  expect(partsOf("hi", "Read {page}.", "page")).toEqual(["Read ", "."]);
});
