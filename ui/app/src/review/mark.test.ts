// @vitest-environment happy-dom
import { expect, test } from "vitest";
import { MARK, markText, patternOf } from "./mark.ts";

const root = (html: string) => {
  const div = document.createElement("div");
  div.innerHTML = html;
  return div;
};
const marked = (el: Element) =>
  [...el.querySelectorAll(`[${MARK}]`)].map((e) => e.tagName.toLowerCase());

test("a slot matches any words in its place, and nothing else does", () => {
  expect(patternOf("Resets {time}").test("Resets 8:00 am")).toBe(true);
  expect(patternOf("Resets {day} {time}").test("Resets 18 October 8:00 am")).toBe(true);
  expect(patternOf("Resets {time}").test("It resets 8:00 am")).toBe(false);
  expect(patternOf("Try again (1.5)").test("Try again (1x5)")).toBe(false);
});

test("the innermost element whose whole text is the string is marked, even split into runs", () => {
  const el = root(
    '<p><time><span>रात 11:59</span> <span lang="hi">बजे रीसेट</span></time></p><b>Answering.</b>',
  );
  expect(markText(el, "{time} बजे रीसेट")).toBe(1);
  expect(marked(el)).toEqual(["time"]);
  expect(markText(el, "Answering.")).toBe(1);
});

test("a string held in a longer line is marked only as whole words", () => {
  const el = root("<p>Needs your permission</p><h3>Needs you</h3><span>Sent. Needs you now</span>");
  expect(markText(el, "Needs you")).toBe(2);
  expect(marked(el)).toEqual(["h3", "span"]);
});

test("an accessible name or tooltip counts, and a string that isn't there marks nothing", () => {
  const el = root('<section aria-label="Your question"><p>What?</p></section><i title="Stop"></i>');
  expect(markText(el, "Your question")).toBe(1);
  expect(markText(el, "Stop")).toBe(1);
  expect(markText(el, "Usage limit reached")).toBe(0);
});
