// The web sources' words: the summary's count and its singular, the query's quotes split at its slot, the few muted
// words, and the Hindi standing in as English until #285.
import { expect, test } from "vitest";
import { partsOf, sourcesSummary, src } from "./words-sources.ts";

test("the summary counts the distinct pages, the singular apart", () => {
  expect(sourcesSummary("en", 5)).toBe("5 sources");
  expect(sourcesSummary("en", 1)).toBe("1 source");
  expect(sourcesSummary("en", 2)).toBe("2 sources");
});

test("a query is a quoted run, and the rows' other words are a few, with no period", () => {
  expect(partsOf("en", "'{query}'", "query")).toEqual(["'", "'"]);
  expect(src("en", "No results")).toBe("No results");
  expect(src("en", "{name} didn't say which pages it found", { name: "ChatGPT" })).toBe(
    "ChatGPT didn't say which pages it found",
  );
  expect(src("en", "Opened on OpenAI's side")).toBe("Opened on OpenAI's side");
  expect(src("en", "Opened in the browser")).toBe("Opened in the browser");
});

test("the whole set: no sentence of round 2 is left", () => {
  expect(Object.keys(src.hi).sort()).toEqual(
    [
      "1 source",
      "{n} sources",
      "'{query}'",
      "No results",
      "{name} didn't say which pages it found",
      "Opened on OpenAI's side",
      "Opened in the browser",
    ].sort(),
  );
});

test("every string has its Hindi, and the Hindi interface shows it", () => {
  expect(Object.values(src.hi).every((hi) => hi !== null)).toBe(true);
  expect(sourcesSummary("hi", 2)).toBe("2 स्रोत");
  expect(partsOf("hi", "'{query}'", "query")).toEqual(["'", "'"]);
});
