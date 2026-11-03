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
  expect(src("en", "{name} doesn't list its pages", { name: "ChatGPT" })).toBe(
    "ChatGPT doesn't list its pages",
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
      "{name} doesn't list its pages",
      "Opened on OpenAI's side",
      "Opened in the browser",
      "Searches",
    ].sort(),
  );
});

test("every string is hi: later, and the Hindi interface shows its English", () => {
  expect(Object.values(src.hi).every((hi) => hi === null)).toBe(true);
  expect(sourcesSummary("hi", 2)).toBe("2 sources");
  expect(partsOf("hi", "'{query}'", "query")).toEqual(["'", "'"]);
});
