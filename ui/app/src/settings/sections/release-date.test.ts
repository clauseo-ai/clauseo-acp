import { afterEach, expect, test } from "vitest";
import { releaseDateWords } from "./release-date.ts";

// the zone the local clock reads in: the day before, in New York; the formatter must not read it
const zone = process.env["TZ"];
afterEach(() => {
  if (zone === undefined) delete process.env["TZ"];
  else process.env["TZ"] = zone;
});

test("no date, no line", () => {
  expect(releaseDateWords(null, "en")).toBeNull();
  expect(releaseDateWords(null, "hi")).toBeNull();
});

test("the release date reads in long form in each language, with Latin digits", () => {
  expect(releaseDateWords("2026-11-03", "en")).toBe("3 November 2026");
  expect(releaseDateWords("2026-11-03", "hi")).toBe("3 नवंबर 2026");
});

test("2026-01-01 stays 1 January in a zone behind UTC, where the local clock reads 31 December", () => {
  process.env["TZ"] = "America/New_York";
  // the guard: the local clock here is a day behind, so a local-time reading would show 31 December
  expect(new Date("2026-01-01T00:00:00Z").getDate()).toBe(31);
  expect(releaseDateWords("2026-01-01", "en")).toBe("1 January 2026");
  expect(releaseDateWords("2026-01-01", "hi")).toBe("1 जनवरी 2026");
});

test("a string that is not a YYYY-MM-DD day shows no line", () => {
  for (const bad of [
    "",
    "2026-1-3",
    "03-11-2026",
    "2026-11-03T00:00:00Z",
    "2026-02-30",
    "2026-13-01",
    "v3.0.4",
  ]) {
    expect(releaseDateWords(bad, "en"), bad).toBeNull();
    expect(releaseDateWords(bad, "hi"), bad).toBeNull();
  }
});
