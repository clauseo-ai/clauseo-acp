import { expect, test } from "vitest";
import { SHAPING_ANSWER, SHAPING_CELLS, SHAPING_SECTIONS } from "./shaping-answer.ts";

test("the shaping answer holds every cell verbatim, NFC, and is NFC itself", () => {
  expect(SHAPING_CELLS).toHaveLength(16);
  for (const cell of SHAPING_CELLS) {
    expect(cell.text.normalize("NFC"), `the ${cell.name} cell is not NFC`).toBe(cell.text);
    expect(SHAPING_ANSWER.includes(cell.text), `the ${cell.name} cell is missing`).toBe(true);
  }
  expect(SHAPING_ANSWER.normalize("NFC")).toBe(SHAPING_ANSWER);
  // the precomposed nukta letters, which NFC splits, are not in it
  expect(SHAPING_ANSWER).not.toMatch(/[\u0958-\u095F]/);
});

test("each section is an h2 heading with every block type, and no raw HTML", () => {
  const sections = SHAPING_ANSWER.split(/^## /m).slice(1);
  expect(sections.map((s) => s.slice(0, s.indexOf(":")))).toEqual([...SHAPING_SECTIONS]);
  for (const s of sections) {
    expect(s, s).toMatch(/^- /m);
    expect(s, s).toMatch(/^\| --- \| --- \|$/m);
    expect(s, s).toMatch(/^> /m);
    expect(s, s).toMatch(/^```text$/m);
    expect(s, s).toMatch(/\*\*[^*]+\*\*/);
    expect(s, s).toMatch(/ \*[^*]+\*;/);
    expect(s, s).toMatch(/ `[^`]+`\./);
  }
  expect(SHAPING_ANSWER).not.toMatch(/<[a-z]/i);
});
