// The words tool-line.spec.ts expects are the words the window says: each is a key of words-tools-line.ts or
// words-tools.ts, the line's name once its slots are filled. Each file's keys are its list's English, the keys of the
// HI object in the .hi.ts beside it (words-tools-line.hi.ts, words-tools.hi.ts), read as the module exports them, so
// the check holds whatever each key's Hindi is and however the list is laid out.

import { expect, test } from "@playwright/test";
import { HI as POPOVER_HI } from "../../components/src/lib/words-tools.hi.ts";
import { HI as LINE_HI } from "../../components/src/lib/words-tools-line.hi.ts";
import { BROWSER_REASONS, OFF, ON, WEB_ON } from "./tool-line-words.ts";

const line: string[] = Object.keys(LINE_HI);
const popover: string[] = Object.keys(POPOVER_HI);

test("every line the tool line spec looks for is one the words files have", () => {
  expect(line.length).toBeGreaterThan(5);
  expect(popover.length).toBeGreaterThan(10);
  // the line's name: its sentence template, filled with web search's name and "on"
  expect(line).toContain("{tool}: {state}.");
  expect(line).toContain("Web search");
  expect(line).toContain("on");
  expect("{tool}: {state}.".replace("{tool}", "Web search").replace("{state}", "on")).toBe(WEB_ON);
  expect(popover).toContain(ON);
  expect(popover).toContain(OFF);
  for (const reason of BROWSER_REASONS) expect(line, reason).toContain(reason);
});
