import { readdirSync, readFileSync } from "node:fs";
import { expect, test } from "vitest";
import { clock, t } from "./words.ts";

test("English is the key; Hindi is its value, slots filled in both", () => {
  expect(t("en", "Good law")).toBe("Good law");
  expect(t("hi", "Good law")).toBe("अब भी मान्य");
  expect(t("en", "{n} more", { n: 3 })).toBe("3 more");
  expect(t("hi", "{n} more", { n: 3 })).toBe("3 और");
});

test("Hindi puts the part of the day first", () => {
  expect(clock("hi", "4:33 pm")).toBe("शाम 4:33");
  expect(clock("hi", "3:58 pm")).toBe("दोपहर 3:58");
  expect(clock("hi", "9:40 pm")).toBe("रात 9:40");
  expect(clock("hi", "10:05 am")).toBe("सुबह 10:05");
  expect(clock("en", "4:33 pm")).toBe("4:33 pm");
});

test("the versions view is Versions on screen: no words file says History or इतिहास in a string", () => {
  const dir = new URL(".", import.meta.url);
  const files = readdirSync(dir).filter((f) => /^words.*\.ts$/.test(f) && !f.endsWith(".test.ts"));
  // the scan reads the words files, so a renamed folder can't pass by finding nothing
  expect(files).toContain("words-ui.ts");
  for (const f of files) {
    expect(readFileSync(new URL(f, dir), "utf8").match(/"[^"\n]*(history|इतिहास)/gi), f).toBeNull();
  }
  // the pattern catches the string it should
  expect('"Search covers the papers, the answers and History."').toMatch(
    /"[^"\n]*(history|इतिहास)/i,
  );
});
