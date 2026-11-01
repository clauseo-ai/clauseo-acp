import type { Element, Nodes as Hast } from "hast";
import { expect, test } from "vitest";
import { createAnswer } from "./answer.ts";
import { capNesting, MAX_NESTING } from "./nesting.ts";

// Hostile nesting (PR #107's security review): these inputs overflowed the stack, in marked and in
// mdast-util-to-hast, and unmounted the answer on every render.
const DEEP_LIST = "- ".repeat(1000) + "x";
const DEEP_QUOTE = "> ".repeat(3000) + "x";
/** 600 levels of list built through indentation, which no single line's markers show. */
const INDENTED_LIST = Array.from(
  { length: 20 },
  (_, line) => `${" ".repeat(60 * line)}${"- ".repeat(30)}x\n`,
).join("");
const DEEP_EMPHASIS = "*".repeat(5000) + "a" + "*".repeat(5000);

function count(node: Hast, tagName: string): number {
  const own = node.type === "element" && node.tagName === tagName ? 1 : 0;
  if (!("children" in node)) return own;
  let total = own;
  const stack = [...node.children] as Hast[];
  for (let child = stack.pop(); child; child = stack.pop()) {
    if (child.type === "element" && child.tagName === tagName) total++;
    if ("children" in child) stack.push(...(child.children as Hast[]));
  }
  return total;
}

function textOf(node: Hast): string {
  if (node.type === "text") return node.value;
  return "children" in node ? node.children.map((child) => textOf(child as Hast)).join("") : "";
}

test("a line's container markers past the 32nd are escaped, so the rest of the line is text", () => {
  expect(capNesting(DEEP_LIST)).toBe(
    `${"- ".repeat(MAX_NESTING)}\\${"- ".repeat(1000 - MAX_NESTING)}x`,
  );
  expect(capNesting(DEEP_QUOTE)).toBe(
    `${"> ".repeat(MAX_NESTING)}\\${"> ".repeat(3000 - MAX_NESTING)}x`,
  );
  expect(capNesting(`${">".repeat(40)} x`)).toBe(`${">".repeat(32)}\\${">".repeat(8)} x`);
  expect(capNesting(`${"1. ".repeat(40)}x`)).toBe(`${"1. ".repeat(32)}1\\. ${"1. ".repeat(7)}x`);
  expect(capNesting(`a\n${"> - ".repeat(20)}x\nb`)).toBe(
    `a\n${"> - ".repeat(16)}\\${"> - ".repeat(4)}x\nb`,
  );
});

test("lines within the limit, thematic breaks and markers after text are left alone", () => {
  const text = [
    `${"> ".repeat(MAX_NESTING)}x`,
    "- ".repeat(50),
    "* ".repeat(50),
    `x ${"> ".repeat(50)}`,
    `${"-".repeat(80)}`,
  ].join("\n");
  expect(capNesting(text)).toBe(text);
});

test.each([
  ["1,000 nested list items", DEEP_LIST, "ul", "- - - x"],
  ["3,000 nested block quotes", DEEP_QUOTE, "blockquote", "> > > x"],
])("%s render 32 levels deep, the deeper part as text", (_, text, tagName, rest) => {
  for (const streaming of [true, false]) {
    const [view] = createAnswer("t").update(text, streaming);
    expect(view && count(view.hast, tagName)).toBe(MAX_NESTING);
    expect(view && textOf(view.hast)).toContain(rest);
  }
});

test.each([
  ["600 levels of list through indentation", INDENTED_LIST],
  ["5,000 nested emphasis runs", DEEP_EMPHASIS],
])(
  "a block nested too deeply (%s) shows its source as plain text; the others render",
  (_, deep) => {
    const views = createAnswer("t").update(`Intro **bold**.\n\n${deep}\n\nOutro _end_.\n`, false);
    expect(views).toHaveLength(3);
    const [intro, middle, outro] = views.map((view) => view.hast);
    expect(intro && count(intro, "strong")).toBe(1);
    expect(outro && count(outro, "em")).toBe(1);
    const paragraph = middle?.children[0] as Element | undefined;
    expect(middle?.children).toHaveLength(1);
    expect(paragraph?.tagName).toBe("p");
    expect(paragraph && textOf(paragraph)).toBe(deep.trimEnd());
  },
);
