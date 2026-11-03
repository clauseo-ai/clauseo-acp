// The `/` menu's rules apart from the page: where a trigger stands, what a query matches, the highlight kept by name
// when the list changes, what a pick puts in and how far, the rows' ids, and where the menu stands by the caret.
import { expect, test } from "vitest";
import {
  keepHighlight,
  matching,
  optionIds,
  picked,
  runEnd,
  slashAt,
} from "./composer-menu-logic.ts";

const COMMANDS = [
  { name: "review", description: "Review the changes" },
  { name: "Restart", description: "Start again" },
  { name: "compact", description: "Compact the conversation", input_hint: "what to keep" },
];

test("a slash is a trigger only as its run's first character: the field's start, or after whitespace", () => {
  expect(slashAt("/", 1)).toEqual({ at: 0, query: "" });
  expect(slashAt("/re", 3)).toEqual({ at: 0, query: "re" });
  expect(slashAt("x /re", 5)).toEqual({ at: 2, query: "re" });
  expect(slashAt("line\n/co", 8)).toEqual({ at: 5, query: "co" });
  expect(slashAt("a/", 2)).toBeNull();
  expect(slashAt("and/or", 6)).toBeNull();
  // the caret before the slash, or past a space after it, stands in no trigger
  expect(slashAt("/re", 0)).toBeNull();
  expect(slashAt("/re now", 7)).toBeNull();
  // Devanagari before it, a space between: a trigger
  expect(slashAt("धारा /r", 7)).toEqual({ at: 5, query: "r" });
});

test("a query matches the names it starts, case and Unicode normalisation aside, in the agent's order", () => {
  expect(matching(COMMANDS, "").map((c) => c.name)).toEqual(["review", "Restart", "compact"]);
  expect(matching(COMMANDS, "re").map((c) => c.name)).toEqual(["review", "Restart"]);
  expect(matching(COMMANDS, "RES").map((c) => c.name)).toEqual(["Restart"]);
  expect(matching(COMMANDS, "x")).toEqual([]);
  // a name sent decomposed matches a query typed composed
  const nfd = [{ name: "re\u0301sume\u0301", description: "" }];
  expect(matching(nfd, "\u00e9").length).toBe(0);
  expect(matching(nfd, "r\u00e9s").length).toBe(1);
});

test("the highlight stays on the row of the same name; with that name gone, or none, the first row", () => {
  const next = [COMMANDS[2]!, COMMANDS[0]!];
  expect(keepHighlight(next, "review")).toBe(1);
  expect(keepHighlight(next, "Restart")).toBe(0);
  expect(keepHighlight(next, null)).toBe(0);
});

test("a pick puts the slash and the name in, and a space unless whitespace follows already", () => {
  expect(picked(COMMANDS[0]!, undefined)).toBe("/review ");
  expect(picked(COMMANDS[0]!, "x")).toBe("/review ");
  expect(picked(COMMANDS[0]!, " ")).toBe("/review");
  expect(picked(COMMANDS[0]!, "\n")).toBe("/review");
});

test("a pick's run ends at the next whitespace or the field's end", () => {
  expect(runEnd("/review the lease", 3)).toBe(7);
  expect(runEnd("/re", 3)).toBe(3);
  expect(runEnd("x /re\nview", 5)).toBe(5);
  expect(runEnd("/reवि", 3)).toBe(5);
});

test("the rows' ids come from their names, a name listed twice counted, never from their place", () => {
  expect(optionIds("m", COMMANDS)).toEqual(["m-review", "m-Restart", "m-compact"]);
  expect(optionIds("m", [COMMANDS[2]!, COMMANDS[0]!])).toEqual(["m-compact", "m-review"]);
  const twice = optionIds("m", [COMMANDS[0]!, { name: "review", description: "" }]);
  expect(new Set(twice).size).toBe(2);
  expect(optionIds("m", [{ name: "a b/ç", description: "" }])[0]).not.toMatch(/\s/);
});
